use std::collections::BTreeMap;

use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    AbiParam, Block, Function, InstBuilder, Signature, TrapCode, UserFuncName, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_codegen::verifier::verify_function;
use forge_fir::{
    BinaryOp, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind, FirLocalId,
    FirPlace, FirTerminator, FirUnaryOp, FirValueId, IntWidth, OverflowMode, Ty,
};

use crate::{BackendError, TypeLowering};

pub(crate) fn lower_function(
    fir: &FirFunction,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if !fir.closures.is_empty() {
        return Err(BackendError::UnsupportedFir {
            component: "closures",
        });
    }

    let signature = lower_signature(fir, types, isa)?;
    let mut function = Function::with_name_signature(UserFuncName::user(0, fir.owner.0), signature);

    let mut blocks = BTreeMap::new();
    for fir_block in &fir.blocks {
        let block = function.dfg.make_block();
        function.layout.append_block(block);
        if blocks.insert(fir_block.id, block).is_some() {
            return Err(shape(format!("duplicate FIR block {:?}", fir_block.id)));
        }
    }
    let entry = *blocks
        .get(&fir.entry)
        .ok_or_else(|| shape(format!("missing FIR entry block {:?}", fir.entry)))?;

    let mut parameter_values = BTreeMap::<FirLocalId, Value>::new();
    for local_id in &fir.params {
        let local = fir
            .locals
            .get(local_id)
            .ok_or_else(|| shape(format!("missing FIR parameter local {local_id:?}")))?;
        if !local.parameter {
            return Err(shape(format!(
                "FIR parameter local {local_id:?} is not marked parameter"
            )));
        }
        let value = function
            .dfg
            .append_block_param(entry, types.value_type(&local.ty)?);
        parameter_values.insert(*local_id, value);
    }

    let mut values = BTreeMap::<FirValueId, Value>::new();
    lower_one_block(
        fir,
        fir.entry,
        entry,
        &blocks,
        &parameter_values,
        &mut values,
        types,
        &mut function,
    )?;

    for fir_block in &fir.blocks {
        if fir_block.id == fir.entry {
            continue;
        }
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing CLIF block for {:?}", fir_block.id)))?;
        lower_one_block(
            fir,
            fir_block.id,
            clif_block,
            &blocks,
            &parameter_values,
            &mut values,
            types,
            &mut function,
        )?;
    }

    verify_function(&function, isa).map_err(|errors| BackendError::Cranelift {
        message: format!(
            "CLIF verifier rejected FIR function {:?}: {errors}",
            fir.owner
        ),
    })?;

    Ok(function)
}

fn lower_signature(
    fir: &FirFunction,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Signature, BackendError> {
    let mut signature = Signature::new(CallConv::triple_default(isa.triple()));
    for local_id in &fir.params {
        let local = fir
            .locals
            .get(local_id)
            .ok_or_else(|| shape(format!("missing FIR parameter local {local_id:?}")))?;
        signature
            .params
            .push(AbiParam::new(types.value_type(&local.ty)?));
    }
    if fir.return_type != Ty::Void {
        signature
            .returns
            .push(AbiParam::new(types.value_type(&fir.return_type)?));
    }
    Ok(signature)
}

#[allow(clippy::too_many_arguments)]
fn lower_one_block(
    fir: &FirFunction,
    block_id: FirBlockId,
    clif_block: Block,
    blocks: &BTreeMap<FirBlockId, Block>,
    parameter_values: &BTreeMap<FirLocalId, Value>,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    function: &mut Function,
) -> Result<(), BackendError> {
    let fir_block = fir
        .blocks
        .iter()
        .find(|block| block.id == block_id)
        .ok_or_else(|| shape(format!("missing FIR block {block_id:?}")))?;
    if fir_block.closure.is_some() {
        return Err(BackendError::UnsupportedFir {
            component: "closure blocks",
        });
    }

    let mut cursor = FuncCursor::new(function);
    cursor.goto_bottom(clif_block);
    for instruction in &fir_block.instructions {
        lower_instruction(
            fir,
            instruction,
            parameter_values,
            values,
            types,
            &mut cursor,
        )?;
    }

    let terminator = fir_block
        .terminator
        .as_ref()
        .ok_or_else(|| shape(format!("FIR block {block_id:?} has no terminator")))?;
    lower_terminator(fir, terminator, blocks, values, &mut cursor)
}

fn lower_instruction(
    fir: &FirFunction,
    instruction: &FirInstruction,
    parameter_values: &BTreeMap<FirLocalId, Value>,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result_id = instruction
        .result
        .ok_or(BackendError::UnsupportedInstruction {
            kind: "void instruction",
        })?;
    let result_ty = fir
        .value_types
        .get(&result_id)
        .ok_or_else(|| shape(format!("missing type for FIR value {result_id:?}")))?;

    let value = match &instruction.kind {
        FirInstructionKind::Const { value } => lower_const(value, result_ty, types, cursor)?,
        FirInstructionKind::Load {
            place: FirPlace::Local { local },
        } => *parameter_values
            .get(local)
            .ok_or(BackendError::UnsupportedInstruction {
                kind: "load of non-parameter local",
            })?,
        FirInstructionKind::Unary { op, value } => {
            lower_integer_unary(fir, *op, *value, values, cursor)?
        }
        FirInstructionKind::Binary {
            op,
            left,
            right,
            overflow,
        } => lower_integer_binary(fir, *op, *left, *right, *overflow, values, cursor)?,
        FirInstructionKind::Convert { value, target } => {
            if target != result_ty {
                return Err(shape(format!(
                    "FIR convert target {target:?} does not match result type {result_ty:?}"
                )));
            }
            lower_integer_convert(fir, *value, target, values, types, cursor)?
        }
        FirInstructionKind::Load { .. } => {
            return Err(BackendError::UnsupportedInstruction { kind: "place load" });
        }
        _ => {
            return Err(BackendError::UnsupportedInstruction {
                kind: instruction_kind_name(&instruction.kind),
            });
        }
    };

    let actual = cursor.func.dfg.value_type(value);
    let expected = types.value_type(result_ty)?;
    if actual != expected {
        return Err(shape(format!(
            "FIR value {result_id:?} lowers to CLIF {actual}, expected {expected}"
        )));
    }
    values.insert(result_id, value);
    Ok(())
}

fn lower_integer_unary(
    fir: &FirFunction,
    op: FirUnaryOp,
    input: FirValueId,
    values: &BTreeMap<FirValueId, Value>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let ty = fir
        .value_types
        .get(&input)
        .ok_or_else(|| shape(format!("missing type for unary operand {input:?}")))?;
    if !is_integer_type(ty) {
        return Err(BackendError::UnsupportedInstruction {
            kind: "integer unary operation on non-integer FIR value",
        });
    }
    let value = lookup_value(values, input)?;
    Ok(match op {
        FirUnaryOp::Neg => cursor.ins().ineg(value),
        FirUnaryOp::BitNot => cursor.ins().bnot(value),
        FirUnaryOp::Not => {
            return Err(BackendError::UnsupportedInstruction {
                kind: "logical not is not an integer unary operation",
            });
        }
    })
}

fn lower_integer_binary(
    fir: &FirFunction,
    op: BinaryOp,
    left: FirValueId,
    right: FirValueId,
    overflow: Option<OverflowMode>,
    values: &BTreeMap<FirValueId, Value>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    if is_comparison(op) {
        return lower_comparison(fir, op, left, right, values, cursor);
    }

    let operand_ty = matching_integer_operands(fir, left, right, "binary operation")?;
    let left_value = lookup_value(values, left)?;
    let right_value = lookup_value(values, right)?;

    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
            lower_add_sub_mul(op, operand_ty, overflow, left_value, right_value, cursor)
        }
        BinaryOp::Div | BinaryOp::Rem => {
            if overflow != Some(OverflowMode::Checked) {
                return Err(shape(format!(
                    "FIR {op:?} must carry checked arithmetic semantics"
                )));
            }
            let signed = integer_signed(operand_ty)?;
            Ok(match (op, signed) {
                (BinaryOp::Div, true) => cursor.ins().sdiv(left_value, right_value),
                (BinaryOp::Div, false) => cursor.ins().udiv(left_value, right_value),
                (BinaryOp::Rem, true) => cursor.ins().srem(left_value, right_value),
                (BinaryOp::Rem, false) => cursor.ins().urem(left_value, right_value),
                _ => unreachable!(),
            })
        }
        BinaryOp::ShiftLeft | BinaryOp::ShiftRight => {
            if overflow != Some(OverflowMode::Checked) {
                return Err(shape(format!(
                    "FIR {op:?} must carry checked shift semantics"
                )));
            }
            let bits = cursor.func.dfg.value_type(left_value).bits();
            let out_of_range = cursor.ins().icmp_imm_u(
                IntCC::UnsignedGreaterThanOrEqual,
                right_value,
                bits as i64,
            );
            cursor
                .ins()
                .trapnz(out_of_range, TrapCode::INTEGER_OVERFLOW);
            Ok(match op {
                BinaryOp::ShiftLeft => cursor.ins().ishl(left_value, right_value),
                BinaryOp::ShiftRight if integer_signed(operand_ty)? => {
                    cursor.ins().sshr(left_value, right_value)
                }
                BinaryOp::ShiftRight => cursor.ins().ushr(left_value, right_value),
                _ => unreachable!(),
            })
        }
        BinaryOp::BitAnd | BinaryOp::BitXor | BinaryOp::BitOr => {
            if overflow.is_some() {
                return Err(shape(format!(
                    "bitwise FIR operation {op:?} unexpectedly carries overflow semantics"
                )));
            }
            Ok(match op {
                BinaryOp::BitAnd => cursor.ins().band(left_value, right_value),
                BinaryOp::BitXor => cursor.ins().bxor(left_value, right_value),
                BinaryOp::BitOr => cursor.ins().bor(left_value, right_value),
                _ => unreachable!(),
            })
        }
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "binary operation outside scalar integer C4b",
        }),
    }
}

fn lower_add_sub_mul(
    op: BinaryOp,
    ty: &Ty,
    overflow: Option<OverflowMode>,
    left: Value,
    right: Value,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    match overflow {
        Some(OverflowMode::Wrapping) => Ok(match op {
            BinaryOp::Add => cursor.ins().iadd(left, right),
            BinaryOp::Sub => cursor.ins().isub(left, right),
            BinaryOp::Mul => cursor.ins().imul(left, right),
            _ => unreachable!(),
        }),
        Some(OverflowMode::Checked) => {
            let signed = integer_signed(ty)?;
            let (result, did_overflow) = match (op, signed) {
                (BinaryOp::Add, true) => cursor.ins().sadd_overflow(left, right),
                (BinaryOp::Add, false) => cursor.ins().uadd_overflow(left, right),
                (BinaryOp::Sub, true) => cursor.ins().ssub_overflow(left, right),
                (BinaryOp::Sub, false) => cursor.ins().usub_overflow(left, right),
                (BinaryOp::Mul, true) => cursor.ins().smul_overflow(left, right),
                (BinaryOp::Mul, false) => cursor.ins().umul_overflow(left, right),
                _ => unreachable!(),
            };
            cursor
                .ins()
                .trapnz(did_overflow, TrapCode::INTEGER_OVERFLOW);
            Ok(result)
        }
        None => Err(shape(format!(
            "integer arithmetic operation {op:?} has no FIR overflow mode"
        ))),
    }
}

fn lower_comparison(
    fir: &FirFunction,
    op: BinaryOp,
    left: FirValueId,
    right: FirValueId,
    values: &BTreeMap<FirValueId, Value>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let left_value = lookup_value(values, left)?;
    let right_value = lookup_value(values, right)?;
    let operand_ty = matching_integer_operands(fir, left, right, "comparison")?;
    let cc = comparison_condition(op, operand_ty)?;
    Ok(cursor.ins().icmp(cc, left_value, right_value))
}

fn lower_integer_convert(
    fir: &FirFunction,
    input: FirValueId,
    target: &Ty,
    values: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let source = fir
        .value_types
        .get(&input)
        .ok_or_else(|| shape(format!("missing type for conversion input {input:?}")))?;
    if !is_integer_type(source) || !is_integer_type(target) {
        return Err(BackendError::UnsupportedInstruction {
            kind: "non-integer scalar conversion",
        });
    }

    let value = lookup_value(values, input)?;
    let source_clif = types.value_type(source)?;
    let target_clif = types.value_type(target)?;
    Ok(match source_clif.bits().cmp(&target_clif.bits()) {
        std::cmp::Ordering::Equal => value,
        std::cmp::Ordering::Greater => cursor.ins().ireduce(target_clif, value),
        std::cmp::Ordering::Less if integer_signed(source)? => {
            cursor.ins().sextend(target_clif, value)
        }
        std::cmp::Ordering::Less => cursor.ins().uextend(target_clif, value),
    })
}

fn matching_integer_operands<'a>(
    fir: &'a FirFunction,
    left: FirValueId,
    right: FirValueId,
    operation: &str,
) -> Result<&'a Ty, BackendError> {
    let left_ty = fir
        .value_types
        .get(&left)
        .ok_or_else(|| shape(format!("missing type for {operation} operand {left:?}")))?;
    let right_ty = fir
        .value_types
        .get(&right)
        .ok_or_else(|| shape(format!("missing type for {operation} operand {right:?}")))?;
    if left_ty != right_ty {
        return Err(shape(format!(
            "{operation} operands have different FIR types: {left_ty:?} and {right_ty:?}"
        )));
    }
    if !is_integer_type(left_ty) {
        return Err(BackendError::UnsupportedInstruction {
            kind: "integer operation on non-integer FIR values",
        });
    }
    Ok(left_ty)
}

fn is_integer_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Byte | Ty::Int { .. })
}

fn integer_signed(ty: &Ty) -> Result<bool, BackendError> {
    match ty {
        Ty::Byte => Ok(false),
        Ty::Int { signed, .. } => Ok(*signed),
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "signedness requested for non-integer FIR value",
        }),
    }
}

fn lower_terminator(
    fir: &FirFunction,
    terminator: &FirTerminator,
    blocks: &BTreeMap<FirBlockId, Block>,
    values: &BTreeMap<FirValueId, Value>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    match terminator {
        FirTerminator::Goto { target } => {
            cursor.ins().jump(block(blocks, *target)?, &[]);
        }
        FirTerminator::Branch {
            condition,
            then_block,
            else_block,
        } => {
            if fir.value_types.get(condition) != Some(&Ty::Bool) {
                return Err(shape(format!("branch condition {condition:?} is not bool")));
            }
            cursor.ins().brif(
                lookup_value(values, *condition)?,
                block(blocks, *then_block)?,
                &[],
                block(blocks, *else_block)?,
                &[],
            );
        }
        FirTerminator::Return { value: Some(value) } => {
            cursor.ins().return_(&[lookup_value(values, *value)?]);
        }
        FirTerminator::Return { value: None } if fir.return_type == Ty::Void => {
            cursor.ins().return_(&[]);
        }
        FirTerminator::Return { value: None } => {
            return Err(shape("non-void FIR function returns no value"));
        }
        FirTerminator::Select { .. } => {
            return Err(BackendError::UnsupportedInstruction {
                kind: "select terminator",
            });
        }
        FirTerminator::Unreachable => {
            return Err(BackendError::UnsupportedInstruction {
                kind: "unreachable terminator",
            });
        }
    }
    Ok(())
}

fn lower_const(
    constant: &FirConst,
    ty: &Ty,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let clif_ty = types.value_type(ty)?;
    match constant {
        FirConst::Integer { text } => {
            let immediate = integer_immediate(text, ty, types)?;
            Ok(cursor.ins().iconst(clif_ty, immediate))
        }
        FirConst::Bool { value } if *ty == Ty::Bool => {
            Ok(cursor.ins().iconst(clif_ty, i64::from(*value)))
        }
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "non-integer scalar constant",
        }),
    }
}

fn integer_immediate(text: &str, ty: &Ty, types: &TypeLowering<'_>) -> Result<i64, BackendError> {
    let cleaned = text.replace('_', "");
    let value = cleaned
        .parse::<i128>()
        .map_err(|_| BackendError::InvalidConstant { text: text.into() })?;
    let (signed, bits) = match ty {
        Ty::Byte => (false, 8_u16),
        Ty::Int { signed, width } => {
            let bits = match width {
                IntWidth::W8 => 8,
                IntWidth::W16 => 16,
                IntWidth::W32 => 32,
                IntWidth::W64 => 64,
                IntWidth::Pointer => types.target().pointer_bits,
            };
            (*signed, bits)
        }
        _ => {
            return Err(shape(format!(
                "integer constant has non-integer FIR type {ty:?}"
            )));
        }
    };

    let fits = if signed {
        let min = -(1_i128 << (bits - 1));
        let max = (1_i128 << (bits - 1)) - 1;
        value >= min && value <= max
    } else {
        let max = (1_i128 << bits) - 1;
        value >= 0 && value <= max
    };
    if !fits {
        return Err(BackendError::InvalidConstant { text: text.into() });
    }
    Ok(value as i64)
}

fn comparison_condition(op: BinaryOp, ty: &Ty) -> Result<IntCC, BackendError> {
    match op {
        BinaryOp::Eq => Ok(IntCC::Equal),
        BinaryOp::NotEq => Ok(IntCC::NotEqual),
        BinaryOp::Less | BinaryOp::LessEq | BinaryOp::Greater | BinaryOp::GreaterEq => {
            let signed = integer_signed(ty)?;
            Ok(match (op, signed) {
                (BinaryOp::Less, true) => IntCC::SignedLessThan,
                (BinaryOp::Less, false) => IntCC::UnsignedLessThan,
                (BinaryOp::LessEq, true) => IntCC::SignedLessThanOrEqual,
                (BinaryOp::LessEq, false) => IntCC::UnsignedLessThanOrEqual,
                (BinaryOp::Greater, true) => IntCC::SignedGreaterThan,
                (BinaryOp::Greater, false) => IntCC::UnsignedGreaterThan,
                (BinaryOp::GreaterEq, true) => IntCC::SignedGreaterThanOrEqual,
                (BinaryOp::GreaterEq, false) => IntCC::UnsignedGreaterThanOrEqual,
                _ => unreachable!(),
            })
        }
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "non-comparison binary operation",
        }),
    }
}

fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq
            | BinaryOp::Eq
            | BinaryOp::NotEq
    )
}

fn lookup_value(
    values: &BTreeMap<FirValueId, Value>,
    id: FirValueId,
) -> Result<Value, BackendError> {
    values
        .get(&id)
        .copied()
        .ok_or_else(|| shape(format!("FIR value {id:?} used before lowering")))
}

fn block(blocks: &BTreeMap<FirBlockId, Block>, id: FirBlockId) -> Result<Block, BackendError> {
    blocks
        .get(&id)
        .copied()
        .ok_or_else(|| shape(format!("missing FIR branch target {id:?}")))
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}

fn instruction_kind_name(kind: &FirInstructionKind) -> &'static str {
    match kind {
        FirInstructionKind::Const { .. } => "const",
        FirInstructionKind::Unit => "unit",
        FirInstructionKind::FunctionRef { .. } => "function ref",
        FirInstructionKind::LoadGlobal { .. } => "global load",
        FirInstructionKind::ContextLoad { .. } => "context load",
        FirInstructionKind::ContextSave { .. } => "context save",
        FirInstructionKind::ContextSet { .. } => "context set",
        FirInstructionKind::ContextRestore { .. } => "context restore",
        FirInstructionKind::Load { .. } => "load",
        FirInstructionKind::Store { .. } => "store",
        FirInstructionKind::Unary { .. } => "unary",
        FirInstructionKind::Binary { .. } => "binary",
        FirInstructionKind::Convert { .. } => "convert",
        FirInstructionKind::BitStructStorage { .. } => "bitstruct storage",
        FirInstructionKind::BitStructFromStorage { .. } => "bitstruct from storage",
        FirInstructionKind::BitFieldCheck { .. } => "bitfield check",
        FirInstructionKind::PointerOffset { .. } => "pointer offset",
        FirInstructionKind::PointerConvert { .. } => "pointer convert",
        FirInstructionKind::MakeArray { .. } => "make array",
        FirInstructionKind::MakeAggregate { .. } => "make aggregate",
        FirInstructionKind::MakeNone => "make none",
        FirInstructionKind::MakeSome { .. } => "make some",
        FirInstructionKind::Variant { .. } => "variant",
        FirInstructionKind::VariantIs { .. } => "variant is",
        FirInstructionKind::ExtractField { .. } => "extract field",
        FirInstructionKind::Len { .. } => "len",
        FirInstructionKind::BoundsCheck { .. } => "bounds check",
        FirInstructionKind::IndexUnchecked { .. } => "index unchecked",
        FirInstructionKind::Subsequence { .. } => "subsequence",
        FirInstructionKind::CollectionPatternLookup { .. } => "collection pattern lookup",
        FirInstructionKind::CollectionPatternHasOnly { .. } => "collection pattern has only",
        FirInstructionKind::AddressOf { .. } => "address of",
        FirInstructionKind::MakeClosure { .. } => "make closure",
        FirInstructionKind::CallClosure { .. } => "closure call",
        FirInstructionKind::Call { .. } => "call",
        FirInstructionKind::CallIndirect { .. } => "indirect call",
        FirInstructionKind::ResultIsOk { .. } => "result is ok",
        FirInstructionKind::ResultUnwrapOk { .. } => "result unwrap ok",
        FirInstructionKind::ResultUnwrapErr { .. } => "result unwrap err",
        FirInstructionKind::MakeResultErr { .. } => "make result err",
        FirInstructionKind::OptionIsSome { .. } => "option is some",
        FirInstructionKind::OptionUnwrap { .. } => "option unwrap",
        FirInstructionKind::Poison => "poison",
    }
}
