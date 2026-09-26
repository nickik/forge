use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::ir::{Function, Signature};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::Context;
use forge_fir::{
    verify_fir_module, BinaryOp, DefId, FirBasicBlock, FirFunction, FirInstructionKind, FirModule,
    FirPlace, FirTerminator, FirValueId, IntWidth, Ty, TypeDefinitionKind, TypeDefinitionTable,
    TypeFieldDefinition, UnsafeOperationKind,
};
use target_lexicon::Triple;

use crate::abi::fir_parameter_types;
use crate::c9_memory_checks::validate_c9_memory_places;
use crate::function::lower_function;
use crate::sia32_privileged_lowering::validate_sia32_privileged_operations;
use crate::{BackendError, CraneliftTarget, TargetLayout, TypeLowering};

/// Target-specific Cranelift state. It deliberately owns no Forge semantic
/// state other than verified FIR and the resolved type-definition table passed
/// to lowering operations.
pub struct CraneliftBackend {
    target: CraneliftTarget,
    layout: TargetLayout,
    isa: OwnedTargetIsa,
}

impl CraneliftBackend {
    pub fn new(target: CraneliftTarget) -> Result<Self, BackendError> {
        let isa = target.isa()?;
        Ok(Self {
            target,
            layout: target.layout(),
            isa,
        })
    }

    pub fn aarch64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Aarch64)
    }

    pub fn riscv64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Riscv64)
    }

    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub const fn target_layout(&self) -> &TargetLayout {
        &self.layout
    }

    pub fn type_lowering(&self) -> TypeLowering<'_> {
        TypeLowering::new(&self.layout)
    }

    pub fn target_triple(&self) -> &Triple {
        self.isa.triple()
    }

    pub fn new_context(&self) -> Context {
        Context::new()
    }

    pub fn new_signature(&self) -> Signature {
        Signature::new(CallConv::triple_default(self.isa.triple()))
    }

    /// Compatibility entry point for scalar-only modules and aggregate modules
    /// that do not contain nominal types.
    pub fn prepare_module(&self, module: &FirModule) -> Result<PreparedModule, BackendError> {
        let definitions = TypeDefinitionTable::new();
        self.prepare_module_with_types(module, &definitions)
    }

    /// Verify FIR and lower every supported function using the resolved Forge
    /// type-definition table as the sole aggregate layout/ABI source.
    pub fn prepare_module_with_types(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<PreparedModule, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        if !module.globals.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "globals",
            });
        }
        if !module.global_initializers.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "global initializers",
            });
        }
        if !module.global_init_order.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "global initializer order",
            });
        }

        let lowering = self.type_lowering();
        let mut functions = BTreeMap::new();
        for (owner, fir) in &module.functions {
            // Preserve all scalar semantic barriers established before C9.
            validate_c4_scalar_contract(fir, &self.layout, definitions)?;
            validate_function_ref_contracts(fir, &module.functions)?;
            validate_c9_memory_places(fir)?;
            validate_sia32_privileged_operations(self.target, fir)?;

            // FIR block IDs remain indexes, but value dependencies are allowed
            // to be non-topological in vector order. Schedule both scalar and
            // aggregate value dependencies before CLIF lowering.
            let scheduled = schedule_value_blocks(fir)?;
            let function = lower_function(
                &scheduled,
                &module.functions,
                definitions,
                &lowering,
                &*self.isa,
            )?;
            functions.insert(*owner, function);
        }

        Ok(PreparedModule {
            target: self.target,
            functions,
        })
    }
}

fn validate_function_ref_contracts(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
) -> Result<(), BackendError> {
    for block in &fir.blocks {
        for instruction in &block.instructions {
            let FirInstructionKind::FunctionRef { target } = &instruction.kind else {
                continue;
            };
            let result = instruction
                .result
                .ok_or_else(|| shape("function-ref has no result"))?;
            let result_ty = value_type(fir, result, "function-ref result")?;
            let Ty::Function {
                params,
                result,
                named_arguments: _,
            } = result_ty
            else {
                return Err(shape(format!(
                    "function-ref has non-function result type {result_ty:?}"
                )));
            };
            let callee = all_functions.get(target).ok_or_else(|| {
                shape(format!("function-ref target {target:?} is not in module"))
            })?;
            let expected_params = fir_parameter_types(callee)?;
            if params != &expected_params || result.as_ref() != &callee.return_type {
                return Err(shape(format!(
                    "function-ref type {result_ty:?} does not match target {target:?} signature"
                )));
            }
        }
    }
    Ok(())
}

fn validate_c4_scalar_contract(
    fir: &FirFunction,
    layout: &TargetLayout,
    definitions: &TypeDefinitionTable,
) -> Result<(), BackendError> {
    for block in &fir.blocks {
        for instruction in &block.instructions {
            if let FirInstructionKind::BitFieldCheck { value, width } = &instruction.kind {
                if instruction.result.is_some() {
                    return Err(shape("bitfield range check unexpectedly has a result"));
                }
                let field_ty = value_type(fir, *value, "bitfield range-check input")?;
                let Some((signed, bits)) = integer_shape(field_ty, layout) else {
                    return Err(shape(
                        "bitfield range check requires an unsigned integer value",
                    ));
                };
                if signed {
                    return Err(shape(
                        "bitfield range check requires an unsigned integer value",
                    ));
                }
                let Ok(width) = u16::try_from(*width) else {
                    return Err(shape("bitfield range check has an invalid field width"));
                };
                if width == 0 || width >= bits {
                    return Err(shape("bitfield range check has an invalid field width"));
                }
            }

            if let FirInstructionKind::BoundsCheck { index, len } = &instruction.kind {
                if instruction.result.is_some() {
                    return Err(shape("bounds-check instruction unexpectedly has a result"));
                }
                let usize_ty = Ty::Int {
                    signed: false,
                    width: forge_fir::IntWidth::Pointer,
                };
                let index_ty = value_type(fir, *index, "bounds-check index")?;
                if index_ty != &usize_ty {
                    return Err(shape(format!(
                        "bounds-check index has non-usize FIR type {index_ty:?}"
                    )));
                }
                let len_ty = value_type(fir, *len, "bounds-check length")?;
                if len_ty != &usize_ty {
                    return Err(shape(format!(
                        "bounds-check length has non-usize FIR type {len_ty:?}"
                    )));
                }
            }

            let Some(result) = instruction.result else {
                continue;
            };
            let result_ty = fir
                .value_types
                .get(&result)
                .ok_or_else(|| shape(format!("missing type for FIR value {result:?}")))?;

            match &instruction.kind {
                FirInstructionKind::Unit => {
                    if result_ty != &Ty::Void {
                        return Err(shape(format!(
                            "unit instruction has non-void FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::MakeNone => {
                    if !matches!(result_ty, Ty::Optional { .. }) {
                        return Err(shape(format!(
                            "make-none instruction has non-optional FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::MakeSome { value } => {
                    let Ty::Optional { inner } = result_ty else {
                        return Err(shape(format!(
                            "make-some instruction has non-optional FIR result type {result_ty:?}"
                        )));
                    };
                    let payload = value_type(fir, *value, "option constructor payload")?;
                    if payload != inner.as_ref() {
                        return Err(shape(format!(
                            "make-some instruction has FIR payload type {payload:?}, optional payload is {inner:?}"
                        )));
                    }
                }
                FirInstructionKind::MakeAggregate {
                    ty,
                    variant,
                    fields,
                } => validate_make_aggregate_contract(
                    fir,
                    definitions,
                    ty,
                    result_ty,
                    variant.as_deref(),
                    fields,
                )?,
                FirInstructionKind::MakeArray { items } => {
                    let Ty::Array {
                        element,
                        length: Some(length),
                    } = result_ty
                    else {
                        return Err(shape(format!(
                            "make-array instruction has non-fixed-array FIR result type {result_ty:?}"
                        )));
                    };
                    if *length != items.len() as u64 {
                        return Err(shape(format!(
                            "make-array instruction declares length {length}, but has {} item(s)",
                            items.len()
                        )));
                    }
                    for item in items {
                        let item_ty = value_type(fir, *item, "array item")?;
                        if item_ty != element.as_ref() {
                            return Err(shape(format!(
                                "make-array item has FIR type {item_ty:?}, array element type is {element:?}"
                            )));
                        }
                    }
                }
                FirInstructionKind::Len { value } => {
                    let source = value_type(fir, *value, "length input")?;
                    if !matches!(
                        source,
                        Ty::Array {
                            length: Some(_),
                            ..
                        } | Ty::Slice { .. }
                            | Ty::Str
                    ) {
                        return Err(shape(format!(
                            "len instruction has unsupported FIR input type {source:?}"
                        )));
                    }
                    let usize_ty = Ty::Int {
                        signed: false,
                        width: forge_fir::IntWidth::Pointer,
                    };
                    if result_ty != &usize_ty {
                        return Err(shape(format!(
                            "len instruction has non-usize FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::IndexUnchecked { base, index } => {
                    let base_ty = value_type(fir, *base, "unchecked-index base")?;
                    let expected = match base_ty {
                        Ty::Array {
                            element,
                            length: Some(_),
                        }
                        | Ty::Slice { element, .. } => element.as_ref().clone(),
                        Ty::Str => Ty::Int {
                            signed: false,
                            width: forge_fir::IntWidth::W8,
                        },
                        _ => {
                            return Err(shape(format!(
                                "index-unchecked instruction has unsupported FIR base type {base_ty:?}"
                            )));
                        }
                    };
                    let usize_ty = Ty::Int {
                        signed: false,
                        width: forge_fir::IntWidth::Pointer,
                    };
                    let index_ty = value_type(fir, *index, "unchecked-index index")?;
                    if index_ty != &usize_ty {
                        return Err(shape(format!(
                            "index-unchecked instruction has non-usize FIR index type {index_ty:?}"
                        )));
                    }
                    if result_ty != &expected {
                        return Err(shape(format!(
                            "index-unchecked instruction has FIR result type {result_ty:?}, indexed element type is {expected:?}"
                        )));
                    }
                }
                FirInstructionKind::Variant { ty, name } => {
                    if ty != result_ty {
                        return Err(shape(format!(
                            "variant instruction declares type {ty:?}, result is {result_ty:?}"
                        )));
                    }
                    if !named_variant_is_fieldless(definitions, ty, name)? {
                        return Err(shape(format!(
                            "variant instruction cannot construct payload-bearing variant `{name}`"
                        )));
                    }
                }
                FirInstructionKind::VariantIs { value, name } => {
                    let source = value_type(fir, *value, "variant test input")?;
                    named_variant_is_fieldless(definitions, source, name)?;
                    if result_ty != &Ty::Bool {
                        return Err(shape(format!(
                            "variant-is instruction has non-bool FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::OptionIsSome { value } => {
                    let source = value_type(fir, *value, "option test input")?;
                    if !matches!(source, Ty::Optional { .. }) {
                        return Err(shape(format!(
                            "option-is-some instruction has non-optional FIR input type {source:?}"
                        )));
                    }
                    if result_ty != &Ty::Bool {
                        return Err(shape(format!(
                            "option-is-some instruction has non-bool FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::OptionUnwrap { value } => {
                    let source = value_type(fir, *value, "option unwrap input")?;
                    let Ty::Optional { inner } = source else {
                        return Err(shape(format!(
                            "option-unwrap instruction has non-optional FIR input type {source:?}"
                        )));
                    };
                    if result_ty != inner.as_ref() {
                        return Err(shape(format!(
                            "option-unwrap instruction has FIR result type {result_ty:?}, optional payload is {inner:?}"
                        )));
                    }
                }
                FirInstructionKind::MakeResultOk { value } => {
                    let Ty::Result { ok, .. } = result_ty else {
                        return Err(shape(format!(
                            "make-result-ok instruction has non-result FIR result type {result_ty:?}"
                        )));
                    };
                    let payload = value_type(fir, *value, "result ok constructor payload")?;
                    if payload != ok.as_ref() {
                        return Err(shape(format!(
                            "make-result-ok instruction has FIR payload type {payload:?}, ok payload is {ok:?}"
                        )));
                    }
                }
                FirInstructionKind::MakeResultErr { error: value } => {
                    let Ty::Result { error, .. } = result_ty else {
                        return Err(shape(format!(
                            "make-result-err instruction has non-result FIR result type {result_ty:?}"
                        )));
                    };
                    let payload = value_type(fir, *value, "result error constructor payload")?;
                    if payload != error.as_ref() {
                        return Err(shape(format!(
                            "make-result-err instruction has FIR payload type {payload:?}, error payload is {error:?}"
                        )));
                    }
                }
                FirInstructionKind::ResultIsOk { value } => {
                    let source = value_type(fir, *value, "result test input")?;
                    if !matches!(source, Ty::Result { .. }) {
                        return Err(shape(format!(
                            "result-is-ok instruction has non-result FIR input type {source:?}"
                        )));
                    }
                    if result_ty != &Ty::Bool {
                        return Err(shape(format!(
                            "result-is-ok instruction has non-bool FIR result type {result_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::ResultUnwrapOk { value } => {
                    let source = value_type(fir, *value, "result ok unwrap input")?;
                    let Ty::Result { ok, .. } = source else {
                        return Err(shape(format!(
                            "result-unwrap-ok instruction has non-result FIR input type {source:?}"
                        )));
                    };
                    if result_ty != ok.as_ref() {
                        return Err(shape(format!(
                            "result-unwrap-ok instruction has FIR result type {result_ty:?}, ok payload is {ok:?}"
                        )));
                    }
                }
                FirInstructionKind::ResultUnwrapErr { value } => {
                    let source = value_type(fir, *value, "result error unwrap input")?;
                    let Ty::Result { error, .. } = source else {
                        return Err(shape(format!(
                            "result-unwrap-err instruction has non-result FIR input type {source:?}"
                        )));
                    };
                    if result_ty != error.as_ref() {
                        return Err(shape(format!(
                            "result-unwrap-err instruction has FIR result type {result_ty:?}, error payload is {error:?}"
                        )));
                    }
                }
                FirInstructionKind::Load {
                    place: FirPlace::Local { local },
                } => {
                    let local_ty = &fir
                        .locals
                        .get(local)
                        .ok_or_else(|| shape(format!("missing FIR local {local:?}")))?
                        .ty;
                    if local_ty != result_ty {
                        return Err(shape(format!(
                            "load of {local:?} has FIR type {result_ty:?}, local is {local_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::Unary {
                    op: forge_fir::FirUnaryOp::Neg,
                    value,
                } => {
                    let input_ty = value_type(fir, *value, "unary operand")?;
                    if !matches!(input_ty, forge_fir::Ty::Float { .. }) {
                        return Err(BackendError::UnsupportedInstruction {
                            kind: "integer negation requires explicit FIR overflow semantics",
                        });
                    }
                }
                FirInstructionKind::Unary { op, value } => {
                    let input_ty = value_type(fir, *value, "unary operand")?;
                    if matches!(op, forge_fir::FirUnaryOp::BitNot) && input_ty != result_ty {
                        return Err(shape(format!(
                            "integer unary result has FIR type {result_ty:?}, operand is {input_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::Binary {
                    op, left, right, ..
                } => {
                    let left_ty = value_type(fir, *left, "binary operand")?;
                    let right_ty = value_type(fir, *right, "binary operand")?;
                    if left_ty != right_ty {
                        return Err(shape(format!(
                            "binary operands have different FIR types: {left_ty:?} and {right_ty:?}"
                        )));
                    }
                    if is_comparison(*op) {
                        if *result_ty != Ty::Bool {
                            return Err(shape(format!(
                                "comparison result {result:?} has non-bool FIR type {result_ty:?}"
                            )));
                        }
                    } else if is_c4_integer_binary(*op) && left_ty != result_ty {
                        return Err(shape(format!(
                            "integer binary result has FIR type {result_ty:?}, operands are {left_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::IntegerToFloat { value, target } => {
                    if target != result_ty {
                        return Err(shape(format!(
                            "FIR integer-to-float target {target:?} does not match result type {result_ty:?}"
                        )));
                    }
                    let source = value_type(fir, *value, "integer-to-float input")?;
                    if !matches!(source, Ty::Int { .. } | Ty::Byte)
                        || !matches!(target, Ty::Float { .. })
                    {
                        return Err(shape(format!(
                            "invalid integer-to-float conversion from {source:?} to {target:?}"
                        )));
                    }
                }
                FirInstructionKind::FloatConvert { value, target } => {
                    if target != result_ty {
                        return Err(shape(format!(
                            "FIR float conversion target {target:?} does not match result type {result_ty:?}"
                        )));
                    }
                    let source = value_type(fir, *value, "float conversion input")?;
                    if !matches!(source, Ty::Float { .. })
                        || !matches!(target, Ty::Float { .. })
                    {
                        return Err(shape(format!(
                            "invalid float conversion from {source:?} to {target:?}"
                        )));
                    }
                }
                FirInstructionKind::LosslessIntegerConvert { value, target } => {
                    if target != result_ty {
                        return Err(shape(format!(
                            "FIR convert target {target:?} does not match result type {result_ty:?}"
                        )));
                    }
                    let source = value_type(fir, *value, "conversion input")?;
                    if !matches!(source, Ty::Int { .. } | Ty::Byte)
                        || !matches!(target, Ty::Int { .. } | Ty::Byte)
                    {
                        return Err(shape(format!(
                            "lossless integer conversion has non-integer FIR endpoint: {source:?} to {target:?}"
                        )));
                    }
                    if !lossless_integer_conversion(source, target, layout)? {
                        return Err(BackendError::UnsupportedInstruction {
                            kind: "lossy integer conversion requires explicit FIR conversion semantics",
                        });
                    }
                }
                FirInstructionKind::PointerConvert {
                    value,
                    target,
                    operation,
                    ..
                } => {
                    if target != result_ty {
                        return Err(shape(format!(
                            "pointer-convert target {target:?} does not match FIR result type {result_ty:?}"
                        )));
                    }
                    let source = value_type(fir, *value, "pointer-convert input")?;
                    let valid = match operation {
                        UnsafeOperationKind::PointerToInteger => {
                            matches!(source, Ty::Pointer { .. })
                                && matches!(target, Ty::Int { .. } | Ty::Byte)
                        }
                        UnsafeOperationKind::IntegerToPointer => {
                            matches!(source, Ty::Int { .. } | Ty::Byte)
                                && matches!(target, Ty::Pointer { .. })
                        }
                        UnsafeOperationKind::PointerReinterpret => {
                            matches!(source, Ty::Pointer { .. })
                                && matches!(target, Ty::Pointer { .. })
                                && source != target
                        }
                        UnsafeOperationKind::RawDereference { .. }
                        | UnsafeOperationKind::PointerOffset { .. } => {
                            return Err(shape(format!(
                                "pointer-convert instruction uses non-conversion operation {operation:?}"
                            )));
                        }
                    };
                    if !valid {
                        return Err(shape(format!(
                            "pointer-convert operation {operation:?} is incompatible with FIR types {source:?} -> {target:?}"
                        )));
                    }
                }
                FirInstructionKind::PointerOffset {
                    pointer, offset, ..
                } => {
                    let pointer_ty = value_type(fir, *pointer, "pointer-offset base")?;
                    if !matches!(pointer_ty, Ty::Pointer { .. }) {
                        return Err(shape(format!(
                            "pointer offset base has non-pointer FIR type {pointer_ty:?}"
                        )));
                    }
                    if result_ty != pointer_ty {
                        return Err(shape(format!(
                            "pointer offset result type {result_ty:?} differs from base type {pointer_ty:?}"
                        )));
                    }
                    let offset_ty = value_type(fir, *offset, "pointer offset")?;
                    if !matches!(offset_ty, Ty::Int { .. } | Ty::Byte) {
                        return Err(shape(format!(
                            "pointer offset has non-integer FIR type {offset_ty:?}"
                        )));
                    }
                }
                FirInstructionKind::DistinctFromUnderlying { value, distinct } => {
                    if result_ty != &Ty::Nominal(*distinct) {
                        return Err(shape(
                            "distinct construction result has the wrong nominal type",
                        ));
                    }
                    let Some(definition) = definitions.get(distinct) else {
                        return Err(shape(format!(
                            "distinct construction has unknown type {distinct:?}"
                        )));
                    };
                    let TypeDefinitionKind::Distinct { underlying } = &definition.kind else {
                        return Err(shape("distinct construction target is not a distinct type"));
                    };
                    if value_type(fir, *value, "distinct construction input")? != underlying {
                        return Err(shape(
                            "distinct construction input differs from its underlying type",
                        ));
                    }
                }
                FirInstructionKind::DistinctToUnderlying { value, distinct } => {
                    if value_type(fir, *value, "distinct extraction input")?
                        != &Ty::Nominal(*distinct)
                    {
                        return Err(shape(
                            "distinct extraction input has the wrong nominal type",
                        ));
                    }
                    let Some(definition) = definitions.get(distinct) else {
                        return Err(shape(format!(
                            "distinct extraction has unknown type {distinct:?}"
                        )));
                    };
                    let TypeDefinitionKind::Distinct { underlying } = &definition.kind else {
                        return Err(shape("distinct extraction source is not a distinct type"));
                    };
                    if result_ty != underlying {
                        return Err(shape(
                            "distinct extraction result differs from its underlying type",
                        ));
                    }
                }
                FirInstructionKind::SliceFromArrayRef { value } => {
                    let source = value_type(fir, *value, "slice conversion input")?;
                    let (source_mutable, source_element) =
                        match source {
                            Ty::Reference { mutable, inner } => match inner.as_ref() {
                                Ty::Array {
                                    element,
                                    length: Some(_),
                                } => (*mutable, element.as_ref()),
                                _ => return Err(shape(
                                    "slice conversion source is not a reference to a fixed array",
                                )),
                            },
                            _ => return Err(shape("slice conversion source is not a reference")),
                        };
                    let Ty::Slice { mutable, element } = result_ty else {
                        return Err(shape("slice conversion result is not a slice"));
                    };
                    if source_element != element.as_ref() || (*mutable && !source_mutable) {
                        return Err(shape(
                            "slice conversion source and result types are incompatible",
                        ));
                    }
                }
                FirInstructionKind::BitStructStorage { value, storage } => {
                    let source = value_type(fir, *value, "bitstruct storage input")?;
                    let Ty::Nominal(owner) = source else {
                        return Err(shape(
                            "bitstruct storage projection source is not nominal",
                        ));
                    };
                    if result_ty != storage {
                        return Err(shape(
                            "bitstruct storage projection result type differs from storage",
                        ));
                    }
                    let definition = definitions.get(owner).ok_or_else(|| {
                        shape(format!(
                            "bitstruct storage projection has unknown type {owner:?}"
                        ))
                    })?;
                    let TypeDefinitionKind::BitStruct { storage: declared } = &definition.kind
                    else {
                        return Err(shape(
                            "bitstruct storage projection source is not a bitstruct",
                        ));
                    };
                    if declared != storage {
                        return Err(shape(
                            "bitstruct storage projection uses the wrong storage type",
                        ));
                    }
                }
                FirInstructionKind::BitStructFromStorage { value, bitstruct } => {
                    if result_ty != &Ty::Nominal(*bitstruct) {
                        return Err(shape(
                            "bitstruct rebuild result has the wrong nominal type",
                        ));
                    }
                    let definition = definitions.get(bitstruct).ok_or_else(|| {
                        shape(format!("bitstruct rebuild has unknown type {bitstruct:?}"))
                    })?;
                    let TypeDefinitionKind::BitStruct { storage } = &definition.kind else {
                        return Err(shape("bitstruct rebuild target is not a bitstruct"));
                    };
                    if value_type(fir, *value, "bitstruct rebuild input")? != storage {
                        return Err(shape("bitstruct rebuild input differs from storage type"));
                    }
                }
                FirInstructionKind::BitFieldExtract { value } => {
                    let source = value_type(fir, *value, "bitfield conversion input")?;
                    if !matches!(source, Ty::Byte | Ty::Int { signed: false, .. })
                        || !matches!(result_ty, Ty::Byte | Ty::Int { signed: false, .. })
                    {
                        return Err(shape(
                            "bitfield conversion requires unsigned integer FIR types",
                        ));
                    }
                    let source_bits = integer_shape(source, layout)
                        .expect("unsigned integer source has a shape")
                        .1;
                    let result_bits = integer_shape(result_ty, layout)
                        .expect("unsigned integer result has a shape")
                        .1;
                    if source_bits < result_bits {
                        return Err(shape("bitfield extract widens its storage value"));
                    }
                }
                FirInstructionKind::BitFieldExtend { value } => {
                    let source = value_type(fir, *value, "bitfield conversion input")?;
                    if !matches!(source, Ty::Bool | Ty::Byte | Ty::Int { signed: false, .. })
                        || !matches!(result_ty, Ty::Byte | Ty::Int { signed: false, .. })
                    {
                        return Err(shape(
                            "bitfield extension requires a bool or unsigned integer input and unsigned storage",
                        ));
                    }
                    if source != &Ty::Bool {
                        let source_bits = integer_shape(source, layout)
                            .expect("unsigned integer source has a shape")
                            .1;
                        let result_bits = integer_shape(result_ty, layout)
                            .expect("unsigned integer result has a shape")
                            .1;
                        if source_bits > result_bits {
                            return Err(shape("bitfield extend narrows its field value"));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_make_aggregate_contract(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    declared_ty: &Ty,
    result_ty: &Ty,
    variant_name: Option<&str>,
    fields: &[(String, FirValueId)],
) -> Result<(), BackendError> {
    if declared_ty != result_ty {
        return Err(shape(format!(
            "make-aggregate instruction declares type {declared_ty:?}, result is {result_ty:?}"
        )));
    }
    let Ty::Nominal(owner) = declared_ty else {
        return Err(shape(format!(
            "make-aggregate instruction has non-nominal FIR type {declared_ty:?}"
        )));
    };
    let definition = definitions.get(owner).ok_or_else(|| {
        shape(format!(
            "make-aggregate instruction has unknown type {owner:?}"
        ))
    })?;
    let declared_fields: &[TypeFieldDefinition] = match &definition.kind {
        TypeDefinitionKind::Struct {
            fields: declared_fields,
        } => {
            if let Some(name) = variant_name {
                return Err(shape(format!(
                    "struct aggregate instruction unexpectedly names variant `{name}`"
                )));
            }
            declared_fields
        }
        TypeDefinitionKind::Tagged { variants } => {
            let name = variant_name
                .ok_or_else(|| shape("tagged aggregate instruction is missing its variant"))?;
            &variants
                .iter()
                .find(|variant| variant.name == name)
                .ok_or_else(|| {
                    shape(format!(
                        "unknown aggregate variant `{name}` for FIR type {declared_ty:?}"
                    ))
                })?
                .fields
        }
        _ => {
            return Err(shape(format!(
                "make-aggregate instruction has non-aggregate FIR type {declared_ty:?}"
            )))
        }
    };

    let mut seen = BTreeSet::new();
    for (name, value) in fields {
        if !seen.insert(name.as_str()) {
            return Err(shape(format!("duplicate aggregate field `{name}`")));
        }
        let declared = declared_fields
            .iter()
            .find(|field| field.name == *name)
            .ok_or_else(|| shape(format!("unknown aggregate field `{name}`")))?;
        let payload = value_type(fir, *value, "aggregate field payload")?;
        if payload != &declared.ty {
            return Err(shape(format!(
                "make-aggregate field `{name}` has FIR payload type {payload:?}, declared field type is {:?}",
                declared.ty
            )));
        }
    }
    for declared in declared_fields {
        if !seen.contains(declared.name.as_str()) {
            return Err(shape(format!(
                "make-aggregate instruction is missing declared field `{}`",
                declared.name
            )));
        }
    }
    Ok(())
}

fn named_variant_is_fieldless(
    definitions: &TypeDefinitionTable,
    ty: &Ty,
    name: &str,
) -> Result<bool, BackendError> {
    let Ty::Nominal(owner) = ty else {
        return Err(shape(format!(
            "named variant instruction has non-nominal FIR type {ty:?}"
        )));
    };
    let definition = definitions
        .get(owner)
        .ok_or_else(|| shape(format!("named variant instruction has unknown type {owner:?}")))?;
    let variants = match &definition.kind {
        TypeDefinitionKind::Enum { variants } | TypeDefinitionKind::Tagged { variants } => variants,
        _ => {
            return Err(shape(format!(
                "named variant instruction has non-sum FIR type {ty:?}"
            )))
        }
    };
    let variant = variants
        .iter()
        .find(|variant| variant.name == name)
        .ok_or_else(|| shape(format!("unknown variant `{name}` for FIR type {ty:?}")))?;
    Ok(variant.fields.is_empty())
}

fn lossless_integer_conversion(
    source: &Ty,
    target: &Ty,
    layout: &TargetLayout,
) -> Result<bool, BackendError> {
    if source == target {
        return Ok(true);
    }
    let Some((source_signed, source_bits)) = integer_shape(source, layout) else {
        return Ok(false);
    };
    let Some((target_signed, target_bits)) = integer_shape(target, layout) else {
        return Ok(false);
    };

    if target_bits <= source_bits {
        return Ok(false);
    }

    Ok(match (source_signed, target_signed) {
        (true, true) | (false, false) | (false, true) => true,
        (true, false) => false,
    })
}

fn integer_shape(ty: &Ty, layout: &TargetLayout) -> Option<(bool, u16)> {
    match ty {
        Ty::Byte => Some((false, 8)),
        Ty::Int { signed, width } => Some((
            *signed,
            match width {
                IntWidth::W8 => 8,
                IntWidth::W16 => 16,
                IntWidth::W32 => 32,
                IntWidth::W64 => 64,
                IntWidth::Pointer => layout.pointer_bits,
            },
        )),
        _ => None,
    }
}

fn schedule_value_blocks(fir: &FirFunction) -> Result<FirFunction, BackendError> {
    let mut scheduled = fir.clone();
    let entry = fir
        .blocks
        .iter()
        .find(|block| block.id == fir.entry)
        .ok_or_else(|| shape(format!("missing FIR entry block {:?}", fir.entry)))?;

    let mut output = vec![entry.clone()];
    let mut available = BTreeSet::new();
    record_results(entry, &mut available);
    let mut pending: Vec<&FirBasicBlock> = fir
        .blocks
        .iter()
        .filter(|block| block.id != fir.entry)
        .collect();

    while !pending.is_empty() {
        let mut next = Vec::new();
        let mut progressed = false;
        for block in pending {
            if block_ready(block, &available) {
                record_results(block, &mut available);
                output.push(block.clone());
                progressed = true;
            } else {
                next.push(block);
            }
        }
        if !progressed {
            return Err(BackendError::UnsupportedControlFlow {
                feature: "cyclic or merge value dependencies requiring block arguments",
            });
        }
        pending = next;
    }

    scheduled.blocks = output;
    Ok(scheduled)
}

fn block_ready(block: &FirBasicBlock, outer: &BTreeSet<FirValueId>) -> bool {
    let mut available = outer.clone();
    for instruction in &block.instructions {
        let ready = match &instruction.kind {
            FirInstructionKind::Const { .. }
            | FirInstructionKind::Unit
            | FirInstructionKind::FunctionRef { .. }
            | FirInstructionKind::LoadGlobal { .. }
            | FirInstructionKind::AddressOfGlobal { .. }
            | FirInstructionKind::ContextLoad { .. }
            | FirInstructionKind::ContextSave { .. }
            | FirInstructionKind::MakeNone
            | FirInstructionKind::Variant { .. }
            | FirInstructionKind::Poison => true,

            FirInstructionKind::ContextSet { value, .. }
            | FirInstructionKind::StoreGlobal { value, .. }
            | FirInstructionKind::Unary { value, .. }
            | FirInstructionKind::LosslessIntegerConvert { value, .. }
            | FirInstructionKind::IntegerToFloat { value, .. }
            | FirInstructionKind::FloatConvert { value, .. }
            | FirInstructionKind::DistinctFromUnderlying { value, .. }
            | FirInstructionKind::DistinctToUnderlying { value, .. }
            | FirInstructionKind::SliceFromArrayRef { value }
            | FirInstructionKind::BitStructStorage { value, .. }
            | FirInstructionKind::BitStructFromStorage { value, .. }
            | FirInstructionKind::BitFieldCheck { value, .. }
            | FirInstructionKind::BitFieldExtract { value }
            | FirInstructionKind::BitFieldExtend { value }
            | FirInstructionKind::PointerConvert { value, .. }
            | FirInstructionKind::MakeSome { value }
            | FirInstructionKind::VariantIs { value, .. }
            | FirInstructionKind::ExtractField { base: value, .. }
            | FirInstructionKind::Len { value }
            | FirInstructionKind::Subsequence { base: value, .. }
            | FirInstructionKind::ResultIsOk { value }
            | FirInstructionKind::ResultUnwrapOk { value }
            | FirInstructionKind::ResultUnwrapErr { value }
            | FirInstructionKind::MakeResultOk { value }
            | FirInstructionKind::OptionIsSome { value }
            | FirInstructionKind::OptionUnwrap { value } => available.contains(value),

            FirInstructionKind::CollectionPatternLookup {
                collection, key, ..
            } => available.contains(collection) && available.contains(key),
            FirInstructionKind::CollectionPatternHasOnly {
                collection, keys, ..
            } => available.contains(collection) && keys.iter().all(|key| available.contains(key)),

            FirInstructionKind::ContextRestore { saved, .. } => available.contains(saved),
            FirInstructionKind::Binary { left, right, .. }
            | FirInstructionKind::BoundsCheck {
                index: left,
                len: right,
            }
            | FirInstructionKind::IndexUnchecked {
                base: left,
                index: right,
            } => available.contains(left) && available.contains(right),
            FirInstructionKind::Load { place } | FirInstructionKind::AddressOf { place, .. } => {
                place_ready(place, &available)
            }
            FirInstructionKind::Store { place, value } => {
                available.contains(value) && place_ready(place, &available)
            }
            FirInstructionKind::PointerOffset {
                pointer, offset, ..
            } => available.contains(pointer) && available.contains(offset),
            FirInstructionKind::MakeArray { items } => {
                items.iter().all(|value| available.contains(value))
            }
            FirInstructionKind::MakeAggregate { fields, .. } => {
                fields.iter().all(|(_, value)| available.contains(value))
            }
            FirInstructionKind::MakeClosure { captures, .. } => {
                captures.iter().all(|value| available.contains(value))
            }
            FirInstructionKind::CallClosure { closure, args, .. } => {
                available.contains(closure) && args.iter().all(|value| available.contains(value))
            }
            FirInstructionKind::Call { args, .. } => {
                args.iter().all(|value| available.contains(value))
            }
            FirInstructionKind::CallIndirect { callee, args, .. } => {
                available.contains(callee) && args.iter().all(|value| available.contains(value))
            }
            FirInstructionKind::MakeResultErr { error } => available.contains(error),
            FirInstructionKind::Sia32Privileged { args, .. } => {
                args.iter().all(|value| available.contains(value))
            }
        };
        if !ready {
            return false;
        }
        if let Some(result) = instruction.result {
            available.insert(result);
        }
    }
    match block.terminator.as_ref() {
        Some(FirTerminator::Branch { condition, .. }) => available.contains(condition),
        Some(FirTerminator::Return { value: Some(value) }) => available.contains(value),
        _ => true,
    }
}

fn place_ready(place: &FirPlace, available: &BTreeSet<FirValueId>) -> bool {
    match place {
        FirPlace::Local { .. } | FirPlace::ClosureCapture { .. } => true,
        FirPlace::Field { base, .. } => place_ready(base, available),
        FirPlace::Index { base, index } => {
            place_ready(base, available) && available.contains(index)
        }
        FirPlace::Deref { address } | FirPlace::RawDeref { address, .. } => {
            available.contains(address)
        }
    }
}

fn record_results(block: &FirBasicBlock, available: &mut BTreeSet<FirValueId>) {
    for instruction in &block.instructions {
        if let Some(result) = instruction.result {
            available.insert(result);
        }
    }
}

fn value_type<'a>(
    fir: &'a FirFunction,
    value: FirValueId,
    role: &str,
) -> Result<&'a Ty, BackendError> {
    fir.value_types
        .get(&value)
        .ok_or_else(|| shape(format!("missing type for {role} {value:?}")))
}

fn is_comparison(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq
    )
}

fn is_c4_integer_binary(op: BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Rem
            | BinaryOp::BitAnd
            | BinaryOp::BitXor
            | BinaryOp::BitOr
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight
    )
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}

pub struct PreparedModule {
    target: CraneliftTarget,
    functions: BTreeMap<DefId, Function>,
}

impl PreparedModule {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn functions(&self) -> &BTreeMap<DefId, Function> {
        &self.functions
    }

    pub fn function(&self, owner: DefId) -> Option<&Function> {
        self.functions.get(&owner)
    }
}
