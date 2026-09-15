use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    Block, ExtFuncData, ExternalName, FuncRef, Function, Inst, InstBuilder, MemFlagsData,
    StackSlot, StackSlotData, StackSlotKind, TrapCode, UserExternalName, UserFuncName, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_codegen::verifier::verify_function;
use forge_fir::{
    BinaryOp, DefId, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocalId, FirPlace, FirTerminator, FirUnaryOp, FirValueId, IntWidth, OverflowMode, Ty,
};

use crate::abi::{fir_parameter_types, lower_fir_signature, lower_function_type_signature};
use crate::{BackendError, TypeLowering};

#[derive(Clone, Copy)]
struct MemoryFlags {
    stack: MemFlagsData,
    deref: MemFlagsData,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlaceAccess {
    Load,
    Store,
}

pub(crate) fn lower_function(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if !fir.closures.is_empty() {
        return Err(BackendError::UnsupportedFir {
            component: "closures",
        });
    }

    let call_conv = CallConv::triple_default(isa.triple());
    let signature = lower_fir_signature(fir, types, call_conv)?;
    let mut function = Function::with_name_signature(UserFuncName::user(0, fir.owner.0), signature);

    let local_slots = allocate_addressable_locals(fir, types, &mut function)?;
    let memory_flags = MemoryFlags {
        stack: MemFlagsData::trusted(),
        // Dereferences are deliberately conservative: potentially trapping,
        // unaligned, and not freely movable. C7 does not claim volatile access
        // semantics; volatile raw dereferences are rejected below.
        deref: MemFlagsData::new(),
    };

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
    let mut direct_functions = BTreeMap::<DefId, FuncRef>::new();
    lower_one_block(
        fir,
        all_functions,
        fir.entry,
        entry,
        &blocks,
        &parameter_values,
        &local_slots,
        memory_flags,
        call_conv,
        &mut direct_functions,
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
            all_functions,
            fir_block.id,
            clif_block,
            &blocks,
            &parameter_values,
            &local_slots,
            memory_flags,
            call_conv,
            &mut direct_functions,
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

fn allocate_addressable_locals(
    fir: &FirFunction,
    types: &TypeLowering<'_>,
    function: &mut Function,
) -> Result<BTreeMap<FirLocalId, StackSlot>, BackendError> {
    let mut required = BTreeSet::new();
    for block in &fir.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                FirInstructionKind::Load {
                    place: FirPlace::Local { local },
                }
                | FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    ..
                }
                | FirInstructionKind::AddressOf {
                    place: FirPlace::Local { local },
                    ..
                } => {
                    required.insert(*local);
                }
                _ => {}
            }
        }
    }

    let mut slots = BTreeMap::new();
    for local_id in required {
        let local = fir
            .locals
            .get(&local_id)
            .ok_or_else(|| shape(format!("missing FIR local {local_id:?}")))?;
        let layout = types.scalar_layout(&local.ty)?;
        let slot = function.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            layout.size_bytes,
            layout.align_shift(),
        ));
        slots.insert(local_id, slot);
    }
    Ok(slots)
}

#[allow(clippy::too_many_arguments)]
fn lower_one_block(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    block_id: FirBlockId,
    clif_block: Block,
    blocks: &BTreeMap<FirBlockId, Block>,
    parameter_values: &BTreeMap<FirLocalId, Value>,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    memory_flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
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

    if block_id == fir.entry {
        initialize_parameter_slots(
            parameter_values,
            local_slots,
            memory_flags.stack,
            types,
            &mut cursor,
        )?;
    }

    for instruction in &fir_block.instructions {
        lower_instruction(
            fir,
            all_functions,
            instruction,
            local_slots,
            memory_flags,
            call_conv,
            direct_functions,
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

fn initialize_parameter_slots(
    parameter_values: &BTreeMap<FirLocalId, Value>,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    stack_flags: MemFlagsData,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let pointer_ty = types.pointer_type()?;
    for (local, value) in parameter_values {
        let Some(slot) = local_slots.get(local) else {
            continue;
        };
        let address = cursor.ins().stack_addr(pointer_ty, *slot, 0);
        cursor.ins().store(stack_flags, *value, address, 0);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_instruction(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    instruction: &FirInstruction,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    memory_flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if let FirInstructionKind::Store { place, value } = &instruction.kind {
        if instruction.result.is_some() {
            return Err(shape("FIR store unexpectedly produces a value"));
        }
        lower_store(
            fir,
            place,
            *value,
            local_slots,
            memory_flags,
            values,
            types,
            cursor,
        )?;
        return Ok(());
    }

    match &instruction.kind {
        FirInstructionKind::Call { target, args, tail } => {
            if *tail {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "required tail call",
                });
            }
            return lower_direct_call_instruction(
                fir,
                all_functions,
                instruction,
                *target,
                args,
                call_conv,
                direct_functions,
                values,
                types,
                cursor,
            );
        }
        FirInstructionKind::CallIndirect { callee, args, tail } => {
            if *tail {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "required tail call",
                });
            }
            return lower_indirect_call_instruction(
                fir,
                instruction,
                *callee,
                args,
                call_conv,
                values,
                types,
                cursor,
            );
        }
        _ => {}
    }

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
        FirInstructionKind::FunctionRef { target } => lower_function_ref(
            *target,
            result_ty,
            all_functions,
            call_conv,
            direct_functions,
            types,
            cursor,
        )?,
        FirInstructionKind::Load { place } => lower_load(
            fir,
            place,
            result_ty,
            local_slots,
            memory_flags,
            values,
            types,
            cursor,
        )?,
        FirInstructionKind::AddressOf { place, mutable } => {
            lower_address_of(fir, place, *mutable, result_ty, local_slots, types, cursor)?
        }
        FirInstructionKind::PointerOffset {
            pointer,
            offset,
            subtract,
            provenance: _,
        } => lower_pointer_offset(
            fir, *pointer, *offset, *subtract, result_ty, values, types, cursor,
        )?,
        FirInstructionKind::Unary { op, value } => {
            lower_integer_unary(fir, *op, *value, values, cursor)?
        }
        FirInstructionKind::Binary {
            op,
            left,
            right,
            overflow,
        } => {
            if matches!(
                op,
                BinaryOp::LogicalAnd | BinaryOp::LogicalXor | BinaryOp::LogicalOr
            ) {
                lower_boolean_binary(fir, *op, *left, *right, *overflow, values, cursor)?
            } else {
                lower_integer_binary(fir, *op, *left, *right, *overflow, values, cursor)?
            }
        }
        FirInstructionKind::Convert { value, target } => {
            if target != result_ty {
                return Err(shape(format!(
                    "FIR convert target {target:?} does not match result type {result_ty:?}"
                )));
            }
            lower_integer_convert(fir, *value, target, values, types, cursor)?
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

#[allow(clippy::too_many_arguments)]
fn lower_direct_call_instruction(
    caller: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    instruction: &FirInstruction,
    target: DefId,
    args: &[FirValueId],
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee = all_functions.get(&target).ok_or_else(|| {
        shape(format!(
            "direct FIR call target {target:?} is not in the module"
        ))
    })?;
    let params = fir_parameter_types(callee)?;
    let lowered_args = lower_call_arguments(caller, args, &params, values)?;
    let func_ref =
        import_direct_function(target, callee, call_conv, direct_functions, types, cursor)?;
    let inst = cursor.ins().call(func_ref, &lowered_args);
    record_call_result(
        caller,
        instruction,
        &callee.return_type,
        inst,
        values,
        types,
        cursor,
    )
}

fn lower_indirect_call_instruction(
    caller: &FirFunction,
    instruction: &FirInstruction,
    callee: FirValueId,
    args: &[FirValueId],
    call_conv: CallConv,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee_ty = caller
        .value_types
        .get(&callee)
        .ok_or_else(|| shape(format!("missing type for indirect callee {callee:?}")))?;
    let Ty::Function {
        params,
        result,
        named_arguments: _,
    } = callee_ty
    else {
        return Err(shape(format!(
            "indirect FIR call has non-function callee type {callee_ty:?}"
        )));
    };
    let lowered_args = lower_call_arguments(caller, args, params, values)?;
    let signature = lower_function_type_signature(callee_ty, types, call_conv)?;
    let sig_ref = cursor.func.import_signature(signature);
    let inst = cursor
        .ins()
        .call_indirect(sig_ref, lookup_value(values, callee)?, &lowered_args);
    record_call_result(caller, instruction, result, inst, values, types, cursor)
}

fn lower_call_arguments(
    caller: &FirFunction,
    args: &[FirValueId],
    params: &[Ty],
    values: &BTreeMap<FirValueId, Value>,
) -> Result<Vec<Value>, BackendError> {
    if args.len() != params.len() {
        return Err(shape(format!(
            "FIR call has {} arguments but ABI signature has {} parameters",
            args.len(),
            params.len()
        )));
    }

    args.iter()
        .zip(params)
        .enumerate()
        .map(|(index, (arg, expected_ty))| {
            let actual_ty = caller
                .value_types
                .get(arg)
                .ok_or_else(|| shape(format!("missing type for call argument {arg:?}")))?;
            if actual_ty != expected_ty {
                return Err(shape(format!(
                    "FIR call argument {index} has type {actual_ty:?}, expected {expected_ty:?}"
                )));
            }
            lookup_value(values, *arg)
        })
        .collect()
}

fn record_call_result(
    caller: &FirFunction,
    instruction: &FirInstruction,
    expected_ty: &Ty,
    inst: Inst,
    values: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if *expected_ty == Ty::Void {
        if !cursor.func.dfg.inst_results(inst).is_empty() {
            return Err(shape("void FIR call produced CLIF results"));
        }
        if instruction.result.is_some() {
            return Err(shape("void FIR call unexpectedly has a result value"));
        }
        return Ok(());
    }

    let result_id = instruction
        .result
        .ok_or_else(|| shape("non-void FIR call has no result value"))?;
    let declared = caller
        .value_types
        .get(&result_id)
        .ok_or_else(|| shape(format!("missing type for FIR call value {result_id:?}")))?;
    if declared != expected_ty {
        return Err(shape(format!(
            "FIR call result type {declared:?} does not match callee return type {expected_ty:?}"
        )));
    }

    let result_value = match cursor.func.dfg.inst_results(inst) {
        [value] => *value,
        results => {
            return Err(shape(format!(
                "scalar FIR call expected one CLIF result, got {}",
                results.len()
            )));
        }
    };
    let actual = cursor.func.dfg.value_type(result_value);
    let expected = types.value_type(expected_ty)?;
    if actual != expected {
        return Err(shape(format!(
            "FIR call value {result_id:?} lowers to CLIF {actual}, expected {expected}"
        )));
    }
    values.insert(result_id, result_value);
    Ok(())
}

fn lower_function_ref(
    target: DefId,
    result_ty: &Ty,
    all_functions: &BTreeMap<DefId, FirFunction>,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let callee = all_functions.get(&target).ok_or_else(|| {
        shape(format!(
            "FIR function ref target {target:?} is not in the module"
        ))
    })?;
    let target_params = fir_parameter_types(callee)?;
    let Ty::Function {
        params,
        result,
        named_arguments: _,
    } = result_ty
    else {
        return Err(shape(format!(
            "FIR function ref has non-function result type {result_ty:?}"
        )));
    };
    if params != &target_params || result.as_ref() != &callee.return_type {
        return Err(shape(format!(
            "FIR function ref type {result_ty:?} does not match target {target:?} signature"
        )));
    }

    let func_ref =
        import_direct_function(target, callee, call_conv, direct_functions, types, cursor)?;
    Ok(cursor.ins().func_addr(types.pointer_type()?, func_ref))
}

fn import_direct_function(
    target: DefId,
    callee: &FirFunction,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<FuncRef, BackendError> {
    if let Some(func_ref) = direct_functions.get(&target) {
        return Ok(*func_ref);
    }

    let signature = lower_fir_signature(callee, types, call_conv)?;
    let sig_ref = cursor.func.import_signature(signature);
    let user_name = cursor
        .func
        .declare_imported_user_function(UserExternalName::new(0, target.0));
    let func_ref = cursor.func.import_function(ExtFuncData {
        name: ExternalName::user(user_name),
        signature: sig_ref,
        colocated: true,
        patchable: false,
    });
    direct_functions.insert(target, func_ref);
    Ok(func_ref)
}

#[allow(clippy::too_many_arguments)]
fn lower_load(
    fir: &FirFunction,
    place: &FirPlace,
    result_ty: &Ty,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    memory_flags: MemoryFlags,
    values: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let (address, stored_ty, flags) = lower_place_address(
        fir,
        place,
        PlaceAccess::Load,
        local_slots,
        memory_flags,
        values,
        types,
        cursor,
    )?;
    if &stored_ty != result_ty {
        return Err(shape(format!(
            "FIR load result type {result_ty:?} does not match place type {stored_ty:?}"
        )));
    }
    Ok(cursor
        .ins()
        .load(types.value_type(result_ty)?, flags, address, 0))
}

#[allow(clippy::too_many_arguments)]
fn lower_store(
    fir: &FirFunction,
    place: &FirPlace,
    value_id: FirValueId,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    memory_flags: MemoryFlags,
    values: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let value_ty = fir
        .value_types
        .get(&value_id)
        .ok_or_else(|| shape(format!("missing type for FIR store value {value_id:?}")))?;
    let value = lookup_value(values, value_id)?;
    let (address, stored_ty, flags) = lower_place_address(
        fir,
        place,
        PlaceAccess::Store,
        local_slots,
        memory_flags,
        values,
        types,
        cursor,
    )?;
    if value_ty != &stored_ty {
        return Err(shape(format!(
            "FIR store value type {value_ty:?} does not match place type {stored_ty:?}"
        )));
    }
    cursor.ins().store(flags, value, address, 0);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_place_address(
    fir: &FirFunction,
    place: &FirPlace,
    access: PlaceAccess,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    memory_flags: MemoryFlags,
    values: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Value, Ty, MemFlagsData), BackendError> {
    match place {
        FirPlace::Local { local } => {
            let slot = local_slots
                .get(local)
                .copied()
                .ok_or_else(|| shape(format!("FIR local {local:?} has no C7 stack slot")))?;
            let local_ty = fir
                .locals
                .get(local)
                .ok_or_else(|| shape(format!("missing FIR local {local:?}")))?
                .ty
                .clone();
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            Ok((address, local_ty, memory_flags.stack))
        }
        FirPlace::Deref { address } => {
            let address_ty = fir.value_types.get(address).ok_or_else(|| {
                shape(format!("missing type for dereference address {address:?}"))
            })?;
            let (mutable, inner) = match address_ty {
                Ty::Reference { mutable, inner } => (*mutable, inner.as_ref().clone()),
                _ => {
                    return Err(shape(format!(
                        "safe FIR dereference address has non-reference type {address_ty:?}"
                    )));
                }
            };
            if access == PlaceAccess::Store && !mutable {
                return Err(shape("store through shared FIR reference"));
            }
            Ok((lookup_value(values, *address)?, inner, memory_flags.deref))
        }
        FirPlace::RawDeref {
            address,
            volatile,
            provenance: _,
        } => {
            if *volatile {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "volatile raw dereference",
                });
            }
            let address_ty = fir.value_types.get(address).ok_or_else(|| {
                shape(format!(
                    "missing type for raw dereference address {address:?}"
                ))
            })?;
            let inner = match address_ty {
                Ty::Pointer { inner, .. } => inner.as_ref().clone(),
                _ => {
                    return Err(shape(format!(
                        "raw FIR dereference address has non-pointer type {address_ty:?}"
                    )));
                }
            };
            Ok((lookup_value(values, *address)?, inner, memory_flags.deref))
        }
        FirPlace::ClosureCapture { .. } => Err(BackendError::UnsupportedInstruction {
            kind: "closure capture place",
        }),
        FirPlace::Field { .. } | FirPlace::Index { .. } => {
            Err(BackendError::UnsupportedInstruction {
                kind: "aggregate/index place before C9 layout",
            })
        }
    }
}

fn lower_address_of(
    fir: &FirFunction,
    place: &FirPlace,
    mutable: bool,
    result_ty: &Ty,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let FirPlace::Local { local } = place else {
        return Err(BackendError::UnsupportedInstruction {
            kind: "address of non-local place before aggregate layout",
        });
    };
    let local_data = fir
        .locals
        .get(local)
        .ok_or_else(|| shape(format!("missing FIR local {local:?}")))?;
    if mutable && !local_data.mutable {
        return Err(shape(format!(
            "mutable address requested for immutable FIR local {local:?}"
        )));
    }
    let local_ty = &local_data.ty;
    match result_ty {
        Ty::Reference {
            mutable: result_mutable,
            inner,
        } if *result_mutable == mutable && inner.as_ref() == local_ty => {}
        _ => {
            return Err(shape(format!(
                "address-of result type {result_ty:?} does not match local {local:?} type {local_ty:?}"
            )));
        }
    }
    let slot = local_slots.get(local).copied().ok_or_else(|| {
        shape(format!(
            "addressed FIR local {local:?} has no C7 stack slot"
        ))
    })?;
    Ok(cursor.ins().stack_addr(types.pointer_type()?, slot, 0))
}

fn lower_pointer_offset(
    fir: &FirFunction,
    pointer: FirValueId,
    offset: FirValueId,
    subtract: bool,
    result_ty: &Ty,
    values: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let pointer_ty = fir
        .value_types
        .get(&pointer)
        .ok_or_else(|| shape(format!("missing type for pointer-offset base {pointer:?}")))?;
    let inner = match pointer_ty {
        Ty::Pointer { inner, .. } => inner.as_ref(),
        _ => {
            return Err(shape(format!(
                "pointer offset base has non-pointer FIR type {pointer_ty:?}"
            )));
        }
    };
    if result_ty != pointer_ty {
        return Err(shape(format!(
            "pointer offset result type {result_ty:?} differs from base type {pointer_ty:?}"
        )));
    }

    let offset_ty = fir
        .value_types
        .get(&offset)
        .ok_or_else(|| shape(format!("missing type for pointer offset {offset:?}")))?;
    if !is_integer_type(offset_ty) {
        return Err(shape(format!(
            "pointer offset has non-integer FIR type {offset_ty:?}"
        )));
    }

    // Pointer arithmetic uses Forge scalar layout, not CLIF value width, to
    // determine element stride. Aggregate pointees stay explicitly unsupported
    // until C9 defines their Forge ABI/layout representation.
    let stride = types.scalar_layout(inner)?.size_bytes;
    let pointer_clif = types.pointer_type()?;
    let raw_offset = lookup_value(values, offset)?;
    let normalized_offset =
        normalize_integer_to_type(offset_ty, raw_offset, pointer_clif, types, cursor)?;
    let scaled_offset = if stride == 1 {
        normalized_offset
    } else {
        let stride_value = cursor.ins().iconst(pointer_clif, i64::from(stride));
        cursor.ins().imul(normalized_offset, stride_value)
    };
    let base = lookup_value(values, pointer)?;
    Ok(if subtract {
        cursor.ins().isub(base, scaled_offset)
    } else {
        cursor.ins().iadd(base, scaled_offset)
    })
}

fn normalize_integer_to_type(
    source_ty: &Ty,
    value: Value,
    target_clif: cranelift_codegen::ir::Type,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let source_clif = types.value_type(source_ty)?;
    Ok(match source_clif.bits().cmp(&target_clif.bits()) {
        std::cmp::Ordering::Equal => value,
        std::cmp::Ordering::Greater => cursor.ins().ireduce(target_clif, value),
        std::cmp::Ordering::Less if integer_signed(source_ty)? => {
            cursor.ins().sextend(target_clif, value)
        }
        std::cmp::Ordering::Less => cursor.ins().uextend(target_clif, value),
    })
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

fn lower_boolean_binary(
    fir: &FirFunction,
    op: BinaryOp,
    left: FirValueId,
    right: FirValueId,
    overflow: Option<OverflowMode>,
    values: &BTreeMap<FirValueId, Value>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let left_ty = fir
        .value_types
        .get(&left)
        .ok_or_else(|| shape(format!("missing type for FIR value {left:?}")))?;
    let right_ty = fir
        .value_types
        .get(&right)
        .ok_or_else(|| shape(format!("missing type for FIR value {right:?}")))?;
    if *left_ty != Ty::Bool || *right_ty != Ty::Bool {
        return Err(shape(format!(
            "logical operation {op:?} requires bool operands, found {left_ty:?} and {right_ty:?}"
        )));
    }
    if overflow.is_some() {
        return Err(shape(format!(
            "logical FIR operation {op:?} unexpectedly carries overflow semantics"
        )));
    }
    let left = lookup_value(values, left)?;
    let right = lookup_value(values, right)?;
    Ok(match op {
        BinaryOp::LogicalAnd => cursor.ins().band(left, right),
        BinaryOp::LogicalXor => cursor.ins().bxor(left, right),
        BinaryOp::LogicalOr => cursor.ins().bor(left, right),
        _ => unreachable!(),
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

    normalize_integer_to_type(
        source,
        lookup_value(values, input)?,
        types.value_type(target)?,
        types,
        cursor,
    )
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
            // The frontend uses this only for CFG joins proven unreachable.
            // If violated by malformed FIR, trapping is the conservative executable meaning.
            cursor.ins().trap(TrapCode::INTEGER_OVERFLOW);
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
    let literal = [
        "usize", "isize", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
    ]
    .iter()
    .find_map(|suffix| cleaned.strip_suffix(suffix))
    .unwrap_or(cleaned.as_str());
    let (negative, unsigned) = match literal.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, literal),
    };
    let (radix, digits) = if let Some(rest) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, rest)
    } else if let Some(rest) = unsigned
        .strip_prefix("0b")
        .or_else(|| unsigned.strip_prefix("0B"))
    {
        (2, rest)
    } else if let Some(rest) = unsigned
        .strip_prefix("0o")
        .or_else(|| unsigned.strip_prefix("0O"))
    {
        (8, rest)
    } else {
        (10, unsigned)
    };
    let magnitude = i128::from_str_radix(digits, radix)
        .map_err(|_| BackendError::InvalidConstant { text: text.into() })?;
    let value = if negative { -magnitude } else { magnitude };
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
