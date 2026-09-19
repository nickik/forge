use std::collections::BTreeMap;

use cranelift_codegen::cursor::{Cursor, FuncCursor};
use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::{
    types as clif_types, Block, FuncRef, Function, InstBuilder, MemFlagsData, StackSlot,
    StackSlotData, StackSlotKind, TrapCode, UserFuncName, Value,
};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_codegen::verifier::verify_function;
use forge_fir::{
    DefId, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocalId, FirPlace,
    FirTerminator, FirValueId, Layout, LayoutEngine, LayoutKind, LayoutTarget, SumEncoding, Ty,
    TypeDefinitionKind, TypeDefinitionTable,
};

use crate::abi::lower_fir_signature;
use crate::{BackendError, TypeLowering};

type MemFlags = MemFlagsData;

#[allow(dead_code)]
mod legacy {
    use super::MemFlags;

    include!("function.rs");

    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_scalar_instruction(
        fir: &FirFunction,
        all_functions: &BTreeMap<DefId, FirFunction>,
        instruction: &FirInstruction,
        local_slots: &BTreeMap<FirLocalId, StackSlot>,
        stack_flags: MemFlags,
        deref_flags: MemFlags,
        call_conv: CallConv,
        direct_functions: &mut BTreeMap<DefId, FuncRef>,
        values: &mut BTreeMap<FirValueId, Value>,
        types: &TypeLowering<'_>,
        cursor: &mut FuncCursor<'_>,
    ) -> Result<(), BackendError> {
        lower_instruction(
            fir,
            all_functions,
            instruction,
            local_slots,
            MemoryFlags {
                stack: stack_flags,
                deref: deref_flags,
            },
            call_conv,
            direct_functions,
            values,
            types,
            cursor,
        )
    }

    pub(super) fn initialize_scalar_parameters(
        parameter_values: &BTreeMap<FirLocalId, Value>,
        local_slots: &BTreeMap<FirLocalId, StackSlot>,
        stack_flags: MemFlags,
        types: &TypeLowering<'_>,
        cursor: &mut FuncCursor<'_>,
    ) -> Result<(), BackendError> {
        initialize_parameter_slots(parameter_values, local_slots, stack_flags, types, cursor)
    }

    pub(super) fn lower_scalar_terminator(
        fir: &FirFunction,
        terminator: &FirTerminator,
        blocks: &BTreeMap<FirBlockId, Block>,
        values: &BTreeMap<FirValueId, Value>,
        cursor: &mut FuncCursor<'_>,
    ) -> Result<(), BackendError> {
        lower_terminator(fir, terminator, blocks, values, cursor)
    }
}

#[derive(Clone, Copy)]
struct MemoryFlags {
    stack: MemFlags,
    deref: MemFlags,
}

#[derive(Clone)]
struct AggregateValue {
    address: Value,
    ty: Ty,
}

pub(crate) fn lower_function(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if !needs_aggregate_lowering(fir) {
        return legacy::lower_function(fir, all_functions, types, isa);
    }
    lower_aggregate_function(fir, all_functions, definitions, types, isa)
}

fn is_memory_value(ty: &Ty) -> bool {
    !matches!(
        ty,
        Ty::Bool
            | Ty::Byte
            | Ty::Int { .. }
            | Ty::Float { .. }
            | Ty::Char
            | Ty::Duration
            | Ty::Pointer { .. }
            | Ty::Reference { .. }
            | Ty::Function { .. }
            | Ty::Void
            | Ty::Never
    )
}

fn needs_aggregate_lowering(fir: &FirFunction) -> bool {
    if fir.locals.values().any(|local| is_memory_value(&local.ty))
        || fir.value_types.values().any(is_memory_value)
    {
        return true;
    }
    fir.blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::MakeArray { .. }
                    | FirInstructionKind::MakeAggregate { .. }
                    | FirInstructionKind::MakeNone
                    | FirInstructionKind::MakeSome { .. }
                    | FirInstructionKind::Variant { .. }
                    | FirInstructionKind::VariantIs { .. }
                    | FirInstructionKind::ExtractField { .. }
                    | FirInstructionKind::Len { .. }
                    | FirInstructionKind::BoundsCheck { .. }
                    | FirInstructionKind::IndexUnchecked { .. }
                    | FirInstructionKind::ResultIsOk { .. }
                    | FirInstructionKind::ResultUnwrapOk { .. }
                    | FirInstructionKind::ResultUnwrapErr { .. }
                    | FirInstructionKind::MakeResultErr { .. }
                    | FirInstructionKind::MakeResultOk { .. }
                    | FirInstructionKind::OptionIsSome { .. }
                    | FirInstructionKind::OptionUnwrap { .. }
            ) || matches!(
                &instruction.kind,
                FirInstructionKind::Load {
                    place: FirPlace::Field { .. } | FirPlace::Index { .. }
                } | FirInstructionKind::Store {
                    place: FirPlace::Field { .. } | FirPlace::Index { .. },
                    ..
                } | FirInstructionKind::AddressOf {
                    place: FirPlace::Field { .. } | FirPlace::Index { .. },
                    ..
                }
            )
        })
}

fn lower_aggregate_function(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if !fir.closures.is_empty() {
        return Err(BackendError::UnsupportedFir {
            component: "closures",
        });
    }
    for local_id in &fir.params {
        let local = fir
            .locals
            .get(local_id)
            .ok_or_else(|| shape(format!("missing FIR parameter {local_id:?}")))?;
        if is_memory_value(&local.ty) {
            return Err(BackendError::UnsupportedFir {
                component: "aggregate parameters before C9d",
            });
        }
    }
    if fir.return_type != Ty::Void && is_memory_value(&fir.return_type) {
        return Err(BackendError::UnsupportedFir {
            component: "aggregate returns before C9d",
        });
    }

    let call_conv = CallConv::triple_default(isa.triple());
    let signature = lower_fir_signature(fir, types, call_conv)?;
    let mut function = Function::with_name_signature(UserFuncName::user(0, fir.owner.0), signature);
    let mut layouts =
        LayoutEngine::new(LayoutTarget::new(types.target().pointer_bits), definitions);
    let local_slots = allocate_local_slots(fir, &mut layouts, &mut function)?;
    let flags = MemoryFlags {
        stack: MemFlagsData::trusted(),
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

    let mut parameter_values = BTreeMap::new();
    for local_id in &fir.params {
        let local = fir.locals.get(local_id).expect("parameter checked above");
        let value = function
            .dfg
            .append_block_param(entry, types.value_type(&local.ty)?);
        parameter_values.insert(*local_id, value);
    }

    let mut scalars = BTreeMap::<FirValueId, Value>::new();
    let mut aggregates = BTreeMap::<FirValueId, AggregateValue>::new();
    let mut direct_functions = BTreeMap::<DefId, FuncRef>::new();

    for fir_block in &fir.blocks {
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing CLIF block for {:?}", fir_block.id)))?;
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(clif_block);
        if fir_block.id == fir.entry {
            legacy::initialize_scalar_parameters(
                &parameter_values,
                &local_slots,
                flags.stack,
                types,
                &mut cursor,
            )?;
        }
        for instruction in &fir_block.instructions {
            lower_mixed_instruction(
                fir,
                all_functions,
                definitions,
                instruction,
                &local_slots,
                flags,
                call_conv,
                &mut direct_functions,
                &mut scalars,
                &mut aggregates,
                types,
                &mut layouts,
                &mut cursor,
            )?;
        }
        let terminator = fir_block
            .terminator
            .as_ref()
            .ok_or_else(|| shape(format!("FIR block {:?} has no terminator", fir_block.id)))?;
        if let FirTerminator::Return { value: Some(value) } = terminator {
            if fir.value_types.get(value).is_some_and(is_memory_value) {
                return Err(BackendError::UnsupportedFir {
                    component: "aggregate returns before C9d",
                });
            }
        }
        legacy::lower_scalar_terminator(fir, terminator, &blocks, &scalars, &mut cursor)?;
    }

    verify_function(&function, isa).map_err(|errors| BackendError::Cranelift {
        message: format!(
            "CLIF verifier rejected C9c FIR function {:?}: {errors}",
            fir.owner
        ),
    })?;
    Ok(function)
}

fn allocate_local_slots(
    fir: &FirFunction,
    layouts: &mut LayoutEngine<'_>,
    function: &mut Function,
) -> Result<BTreeMap<FirLocalId, StackSlot>, BackendError> {
    let mut result = BTreeMap::new();
    for (id, local) in &fir.locals {
        let layout = layouts.layout_of(&local.ty).map_err(layout_error)?;
        let size = u32::try_from(layout.size.max(1))
            .map_err(|_| shape("local layout exceeds CLIF stack-slot size"))?;
        let slot = function.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            align_shift(layout.align)?,
        ));
        result.insert(*id, slot);
    }
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn lower_mixed_instruction(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if let FirInstructionKind::Store { place, value } = &instruction.kind {
        let ty = value_type(fir, *value)?;
        if is_memory_value(ty) || place_needs_c9(place) {
            if instruction.result.is_some() {
                return Err(shape("store unexpectedly has a result"));
            }
            let (address, stored_ty, dst_flags) = lower_place_address(
                fir,
                definitions,
                place,
                local_slots,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            if ty != &stored_ty {
                return Err(shape("store value and place types differ"));
            }
            store_typed_value(
                *value,
                ty,
                address,
                dst_flags,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                cursor,
            )?;
            return Ok(());
        }
    }

    if let FirInstructionKind::BoundsCheck { index, len } = &instruction.kind {
        if instruction.result.is_some() {
            return Err(shape("bounds check unexpectedly has a result"));
        }
        let index = scalar(scalars, *index)?;
        let len = scalar(scalars, *len)?;
        if cursor.func.dfg.value_type(index) != cursor.func.dfg.value_type(len) {
            return Err(shape("bounds-check operand widths differ"));
        }
        let failed = cursor
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, index, len);
        cursor.ins().trapnz(failed, TrapCode::HEAP_OUT_OF_BOUNDS);
        return Ok(());
    }

    let result_id = instruction.result;
    let result_ty = result_id.map(|id| value_type(fir, id)).transpose()?;

    match &instruction.kind {
        FirInstructionKind::Const { value: forge_fir::FirConst::String { value } }
            if result_ty == Some(&Ty::Str) =>
        {
            let id = result_id.ok_or_else(|| shape("string constant has no result"))?;
            let result = new_aggregate(&Ty::Str, layouts, types, cursor)?;
            let layout = layouts.layout_of(&Ty::Str).map_err(layout_error)?;
            let LayoutKind::Str { data_offset, len_offset } = layout.kind else {
                return Err(shape("str has non-str layout"));
            };
            // First native string-literal slice: materialize bytes in a local
            // immutable stack object, then build the normal {data,len} str value.
            // This establishes correct str semantics before promoting literals
            // into module .rodata/SIA relocations.
            let byte_count = u32::try_from(value.len())
                .map_err(|_| shape("string literal exceeds CLIF stack-slot size"))?;
            let bytes_slot = cursor.func.create_sized_stack_slot(StackSlotData::new(
                StackSlotKind::ExplicitSlot,
                byte_count.max(1),
                0,
            ));
            let data = cursor.ins().stack_addr(types.pointer_type()?, bytes_slot, 0);
            for (offset, byte) in value.as_bytes().iter().copied().enumerate() {
                let byte_value = cursor.ins().iconst(types.value_type(&Ty::Int {
                    signed: false,
                    width: forge_fir::IntWidth::W8,
                })?, i64::from(byte));
                cursor.ins().store(
                    flags.stack,
                    byte_value,
                    data,
                    i32::try_from(offset).map_err(|_| shape("string literal offset exceeds i32"))?,
                );
            }
            cursor.ins().store(flags.stack, data, result.address, i32_offset(data_offset)?);
            let len = cursor.ins().iconst(types.pointer_type()?, i64::try_from(value.len()).map_err(|_| shape("string literal length exceeds i64"))?);
            cursor.ins().store(flags.stack, len, result.address, i32_offset(len_offset)?);
            aggregates.insert(id, result);
            return Ok(());
        }
        FirInstructionKind::Unit => return Ok(()),
        FirInstructionKind::Load { place }
            if result_ty.is_some_and(is_memory_value) || place_needs_c9(place) =>
        {
            let id = result_id.ok_or_else(|| shape("load has no result"))?;
            let ty = result_ty.expect("result id has type");
            let (address, stored_ty, src_flags) = lower_place_address(
                fir,
                definitions,
                place,
                local_slots,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            if ty != &stored_ty {
                return Err(shape("load result and place types differ"));
            }
            materialize_result(
                id,
                ty,
                address,
                src_flags,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::AddressOf { place, mutable } => {
            let id = result_id.ok_or_else(|| shape("address-of has no result"))?;
            let result_ty = result_ty.expect("address-of result type");
            let (address, stored_ty, _) = lower_place_address(
                fir,
                definitions,
                place,
                local_slots,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            match result_ty {
                Ty::Reference {
                    mutable: result_mutable,
                    inner,
                } if *result_mutable == *mutable && inner.as_ref() == &stored_ty => {}
                _ => return Err(shape("address-of result does not match place type")),
            }
            scalars.insert(id, address);
            return Ok(());
        }
        FirInstructionKind::PointerOffset {
            pointer,
            offset,
            subtract,
            ..
        } if pointer_has_memory_pointee(fir, *pointer)? => {
            let id = result_id.ok_or_else(|| shape("pointer-offset has no result"))?;
            let pointer_ty = value_type(fir, *pointer)?;
            if result_ty != Some(pointer_ty) {
                return Err(shape("pointer-offset result differs from base type"));
            }
            let Ty::Pointer { inner, .. } = pointer_ty else {
                return Err(shape("pointer-offset base is not a pointer"));
            };
            let stride = layouts.layout_of(inner).map_err(layout_error)?.size;
            let normalized = normalize_index(
                value_type(fir, *offset)?,
                scalar(scalars, *offset)?,
                types,
                cursor,
            )?;
            let scaled = scale_index(normalized, stride, types, cursor)?;
            let base = scalar(scalars, *pointer)?;
            let value = if *subtract {
                cursor.ins().isub(base, scaled)
            } else {
                cursor.ins().iadd(base, scaled)
            };
            scalars.insert(id, value);
            return Ok(());
        }
        FirInstructionKind::MakeArray { items } => {
            let id = result_id.ok_or_else(|| shape("make-array has no result"))?;
            lower_make_array(
                fir,
                id,
                result_ty.expect("make-array result type"),
                items,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::MakeAggregate {
            ty,
            variant,
            fields,
        } => {
            let id = result_id.ok_or_else(|| shape("make-aggregate has no result"))?;
            let result_ty = result_ty.expect("make-aggregate result type");
            if ty != result_ty {
                return Err(shape("make-aggregate type differs from result"));
            }
            lower_make_aggregate(
                definitions,
                id,
                result_ty,
                variant.as_deref(),
                fields,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::MakeNone => {
            let id = result_id.ok_or_else(|| shape("make-none has no result"))?;
            let ty = result_ty.expect("make-none result type");
            let value = new_aggregate(ty, layouts, types, cursor)?;
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            write_variant(&layout, 0, value.address, flags.stack, cursor)?;
            aggregates.insert(id, value);
            return Ok(());
        }
        FirInstructionKind::MakeSome { value } => {
            let id = result_id.ok_or_else(|| shape("make-some has no result"))?;
            lower_make_some(
                fir,
                id,
                result_ty.expect("make-some result type"),
                *value,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::Variant { ty, name } => {
            let id = result_id.ok_or_else(|| shape("variant has no result"))?;
            let result_ty = result_ty.expect("variant result type");
            if ty != result_ty {
                return Err(shape("variant type differs from result"));
            }
            let value = new_aggregate(result_ty, layouts, types, cursor)?;
            let layout = layouts.layout_of(result_ty).map_err(layout_error)?;
            let index = variant_index(definitions, result_ty, name)?;
            write_variant(&layout, index, value.address, flags.stack, cursor)?;
            aggregates.insert(id, value);
            return Ok(());
        }
        FirInstructionKind::VariantIs { value, name } => {
            let id = result_id.ok_or_else(|| shape("variant-is has no result"))?;
            let ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            let index = variant_index(definitions, ty, name)?;
            let test = read_variant_test(&layout, index, source.address, flags.stack, cursor)?;
            scalars.insert(id, test);
            return Ok(());
        }
        FirInstructionKind::ExtractField { base, field } => {
            let id = result_id.ok_or_else(|| shape("extract-field has no result"))?;
            let result_ty = result_ty.expect("extract-field result type");
            let base_ty = value_type(fir, *base)?;
            let source = aggregate(aggregates, *base)?;
            let (field_ty, offset) = field_projection(definitions, layouts, base_ty, field)?;
            if &field_ty != result_ty {
                return Err(shape("extract-field result type mismatch"));
            }
            let address = add_offset(source.address, offset, cursor)?;
            materialize_result(
                id,
                result_ty,
                address,
                flags.stack,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::Len { value } => {
            let id = result_id.ok_or_else(|| shape("len has no result"))?;
            let ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let len = lower_len(ty, source.address, flags.stack, layouts, types, cursor)?;
            scalars.insert(id, len);
            return Ok(());
        }
        FirInstructionKind::IndexUnchecked { base, index } => {
            let id = result_id.ok_or_else(|| shape("index has no result"))?;
            let result_ty = result_ty.expect("index result type");
            let base_ty = value_type(fir, *base)?;
            let source = aggregate(aggregates, *base)?;
            let (address, element_ty) = index_address(
                fir,
                base_ty,
                source.address,
                *index,
                flags.stack,
                scalars,
                layouts,
                types,
                cursor,
            )?;
            if &element_ty != result_ty {
                return Err(shape("index result type mismatch"));
            }
            materialize_result(
                id,
                result_ty,
                address,
                flags.stack,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::OptionIsSome { value } => {
            let id = result_id.ok_or_else(|| shape("option-is-some has no result"))?;
            let ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            let test = read_variant_test(&layout, 1, source.address, flags.stack, cursor)?;
            scalars.insert(id, test);
            return Ok(());
        }
        FirInstructionKind::OptionUnwrap { value } => {
            let id = result_id.ok_or_else(|| shape("option-unwrap has no result"))?;
            let result_ty = result_ty.expect("option-unwrap result type");
            let option_ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let layout = layouts.layout_of(option_ty).map_err(layout_error)?;
            let address = payload_address(&layout, source.address, cursor)?;
            materialize_result(
                id,
                result_ty,
                address,
                flags.stack,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::MakeResultErr { error } => {
            let id = result_id.ok_or_else(|| shape("make-result-err has no result"))?;
            lower_make_result_err(
                fir,
                id,
                result_ty.expect("result-err result type"),
                *error,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::MakeResultOk { value } => {
            let id = result_id.ok_or_else(|| shape("make-result-ok has no result"))?;
            lower_make_result_payload(
                fir,
                id,
                result_ty.expect("result-ok result type"),
                *value,
                true,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::ResultIsOk { value } => {
            let id = result_id.ok_or_else(|| shape("result-is-ok has no result"))?;
            let ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            let test = read_variant_test(&layout, 0, source.address, flags.stack, cursor)?;
            scalars.insert(id, test);
            return Ok(());
        }
        FirInstructionKind::ResultUnwrapOk { value }
        | FirInstructionKind::ResultUnwrapErr { value } => {
            let id = result_id.ok_or_else(|| shape("result unwrap has no result"))?;
            let result_ty = result_ty.expect("result unwrap result type");
            let sum_ty = value_type(fir, *value)?;
            let source = aggregate(aggregates, *value)?;
            let layout = layouts.layout_of(sum_ty).map_err(layout_error)?;
            let address = payload_address(&layout, source.address, cursor)?;
            materialize_result(
                id,
                result_ty,
                address,
                flags.stack,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                types,
                cursor,
            )?;
            return Ok(());
        }
        FirInstructionKind::Call { target, args, .. } => {
            let callee = all_functions
                .get(target)
                .ok_or_else(|| shape(format!("missing call target {target:?}")))?;
            if callee.return_type != Ty::Void && is_memory_value(&callee.return_type) {
                return Err(BackendError::UnsupportedFir {
                    component: "aggregate calls before C9d",
                });
            }
            for arg in args {
                if is_memory_value(value_type(fir, *arg)?) {
                    return Err(BackendError::UnsupportedFir {
                        component: "aggregate calls before C9d",
                    });
                }
            }
        }
        FirInstructionKind::CallIndirect { callee, args, .. } => {
            let Ty::Function { params, result, .. } = value_type(fir, *callee)? else {
                return Err(shape("indirect callee is not function typed"));
            };
            if (result.as_ref() != &Ty::Void && is_memory_value(result))
                || params.iter().any(is_memory_value)
            {
                return Err(BackendError::UnsupportedFir {
                    component: "aggregate indirect calls before C9d",
                });
            }
            for arg in args {
                if is_memory_value(value_type(fir, *arg)?) {
                    return Err(BackendError::UnsupportedFir {
                        component: "aggregate indirect calls before C9d",
                    });
                }
            }
        }
        _ if result_ty.is_some_and(is_memory_value) => {
            return Err(BackendError::UnsupportedInstruction {
                kind: "aggregate-producing FIR instruction not implemented by C9c",
            });
        }
        _ => {}
    }

    legacy::lower_scalar_instruction(
        fir,
        all_functions,
        instruction,
        local_slots,
        flags.stack,
        flags.deref,
        call_conv,
        direct_functions,
        scalars,
        types,
        cursor,
    )
}

fn place_needs_c9(place: &FirPlace) -> bool {
    matches!(place, FirPlace::Field { .. } | FirPlace::Index { .. })
}

fn pointer_has_memory_pointee(fir: &FirFunction, id: FirValueId) -> Result<bool, BackendError> {
    Ok(matches!(
        value_type(fir, id)?,
        Ty::Pointer { inner, .. } if is_memory_value(inner)
    ))
}

#[allow(clippy::too_many_arguments)]
fn lower_place_address(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    place: &FirPlace,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    flags: MemoryFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Value, Ty, MemFlags), BackendError> {
    match place {
        FirPlace::Local { local } => {
            let slot = local_slots
                .get(local)
                .copied()
                .ok_or_else(|| shape(format!("missing stack slot for {local:?}")))?;
            let ty = fir
                .locals
                .get(local)
                .ok_or_else(|| shape(format!("missing local {local:?}")))?
                .ty
                .clone();
            Ok((
                cursor.ins().stack_addr(types.pointer_type()?, slot, 0),
                ty,
                flags.stack,
            ))
        }
        FirPlace::Deref { address } => {
            let Ty::Reference { inner, .. } = value_type(fir, *address)? else {
                return Err(shape("safe dereference has non-reference address"));
            };
            Ok((
                scalar(scalars, *address)?,
                inner.as_ref().clone(),
                flags.deref,
            ))
        }
        FirPlace::RawDeref {
            address, volatile: _, ..
        } => {
            // Raw volatile accesses are represented as ordinary non-trapping CLIF
            // loads/stores. Their ordering is already pinned by Forge FIR; do not
            // synthesize a generic CLIF fence here. SIA32 has no architectural
            // memory-fence instruction, and the backend must not invent one for
            // device MMIO.
            let Ty::Pointer { inner, .. } = value_type(fir, *address)? else {
                return Err(shape("raw dereference has non-pointer address"));
            };
            Ok((
                scalar(scalars, *address)?,
                inner.as_ref().clone(),
                flags.deref,
            ))
        }
        FirPlace::Field { base, field } => {
            let (address, base_ty, mem_flags) = lower_place_address(
                fir,
                definitions,
                base,
                local_slots,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            let (field_ty, offset) = field_projection(definitions, layouts, &base_ty, field)?;
            Ok((add_offset(address, offset, cursor)?, field_ty, mem_flags))
        }
        FirPlace::Index { base, index } => {
            let (address, base_ty, mem_flags) = lower_place_address(
                fir,
                definitions,
                base,
                local_slots,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            let (address, element_ty) = index_address(
                fir, &base_ty, address, *index, mem_flags, scalars, layouts, types, cursor,
            )?;
            Ok((address, element_ty, mem_flags))
        }
        FirPlace::ClosureCapture { .. } => Err(BackendError::UnsupportedInstruction {
            kind: "closure capture place",
        }),
    }
}

fn field_projection(
    definitions: &TypeDefinitionTable,
    layouts: &mut LayoutEngine<'_>,
    base_ty: &Ty,
    field_name: &str,
) -> Result<(Ty, u64), BackendError> {
    if *base_ty == Ty::Str {
        let layout = layouts.layout_of(base_ty).map_err(layout_error)?;
        let LayoutKind::Str { data_offset, len_offset } = layout.kind else {
            return Err(shape("str has non-str layout"));
        };
        return match field_name {
            "data" => Ok((
                Ty::Pointer {
                    volatile: false,
                    inner: Box::new(Ty::Int { signed: false, width: forge_fir::IntWidth::W8 }),
                },
                data_offset,
            )),
            "len" => Ok((
                Ty::Int { signed: false, width: forge_fir::IntWidth::Pointer },
                len_offset,
            )),
            _ => Err(shape(format!("unknown str field {field_name:?}"))),
        };
    }
    let Ty::Nominal(owner) = base_ty else {
        return Err(shape(format!("field access on non-nominal {base_ty:?}")));
    };
    let definition = definitions
        .get(owner)
        .ok_or_else(|| shape(format!("missing definition {owner:?}")))?
        .clone();
    let layout = layouts.layout_of(base_ty).map_err(layout_error)?;
    match definition.kind {
        TypeDefinitionKind::Struct { fields } => {
            let source = fields
                .iter()
                .find(|field| field.name == field_name)
                .ok_or_else(|| shape(format!("unknown field `{field_name}`")))?;
            let placed = layout
                .fields
                .iter()
                .find(|field| field.declaration_index == source.declaration_index)
                .ok_or_else(|| shape("field missing from layout"))?;
            Ok((source.ty.clone(), placed.offset))
        }
        TypeDefinitionKind::Alias { target }
        | TypeDefinitionKind::Distinct { underlying: target } => {
            field_projection(definitions, layouts, &target, field_name)
        }
        TypeDefinitionKind::Tagged { variants } => {
            let LayoutKind::Tagged {
                variants: placed_variants,
                ..
            } = &layout.kind
            else {
                return Err(shape("tagged type has non-tagged layout"));
            };
            let mut result: Option<(Ty, u64)> = None;
            for (variant, placed_variant) in variants.iter().zip(placed_variants) {
                let Some(source) = variant.fields.iter().find(|field| field.name == field_name)
                else {
                    continue;
                };
                let placed = placed_variant
                    .fields
                    .iter()
                    .find(|field| field.declaration_index == source.declaration_index)
                    .ok_or_else(|| shape("tagged field missing from layout"))?;
                let candidate = (source.ty.clone(), placed.offset);
                if let Some(previous) = &result {
                    if previous != &candidate {
                        return Err(shape("variant-dependent tagged field representation"));
                    }
                } else {
                    result = Some(candidate);
                }
            }
            result.ok_or_else(|| shape(format!("unknown tagged field `{field_name}`")))
        }
        _ => Err(shape("field access on type without fields")),
    }
}

#[allow(clippy::too_many_arguments)]
fn index_address(
    fir: &FirFunction,
    base_ty: &Ty,
    base_address: Value,
    index: FirValueId,
    base_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Value, Ty), BackendError> {
    let index = normalize_index(
        value_type(fir, index)?,
        scalar(scalars, index)?,
        types,
        cursor,
    )?;
    match base_ty {
        Ty::Array { element, .. } => {
            let layout = layouts.layout_of(base_ty).map_err(layout_error)?;
            let LayoutKind::Array { stride, .. } = layout.kind else {
                return Err(shape("array has non-array layout"));
            };
            let offset = scale_index(index, stride, types, cursor)?;
            Ok((
                cursor.ins().iadd(base_address, offset),
                element.as_ref().clone(),
            ))
        }
        Ty::Slice { element, .. } => {
            let layout = layouts.layout_of(base_ty).map_err(layout_error)?;
            let LayoutKind::Slice { data_offset, .. } = layout.kind else {
                return Err(shape("slice has non-slice layout"));
            };
            let data = cursor.ins().load(
                types.pointer_type()?,
                base_flags,
                base_address,
                i32_offset(data_offset)?,
            );
            let stride = layouts.layout_of(element).map_err(layout_error)?.size;
            let offset = scale_index(index, stride, types, cursor)?;
            Ok((cursor.ins().iadd(data, offset), element.as_ref().clone()))
        }
        Ty::Str => {
            let layout = layouts.layout_of(base_ty).map_err(layout_error)?;
            let LayoutKind::Str { data_offset, .. } = layout.kind else {
                return Err(shape("str has non-str layout"));
            };
            let data = cursor.ins().load(
                types.pointer_type()?,
                base_flags,
                base_address,
                i32_offset(data_offset)?,
            );
            // str indexing is explicitly UTF-8 byte indexing in Forge v1.
            Ok((
                cursor.ins().iadd(data, index),
                Ty::Int { signed: false, width: forge_fir::IntWidth::W8 },
            ))
        }
        _ => Err(shape(format!("index on non-array/slice/str {base_ty:?}"))),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_make_array(
    fir: &FirFunction,
    id: FirValueId,
    ty: &Ty,
    items: &[FirValueId],
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let Ty::Array { element, length } = ty else {
        return Err(shape("make-array result is not array"));
    };
    if *length != Some(items.len() as u64) {
        return Err(shape("make-array length mismatch"));
    }
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    let LayoutKind::Array { stride, .. } = layout.kind else {
        return Err(shape("array has non-array layout"));
    };
    let result = new_aggregate(ty, layouts, types, cursor)?;
    for (index, item) in items.iter().enumerate() {
        let item_ty = value_type(fir, *item)?;
        if item_ty != element.as_ref() {
            return Err(shape("array item type mismatch"));
        }
        let address = add_offset(result.address, stride * index as u64, cursor)?;
        store_typed_value(
            *item,
            element,
            address,
            stack_flags,
            stack_flags,
            scalars,
            aggregates,
            layouts,
            cursor,
        )?;
    }
    aggregates.insert(id, result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_make_aggregate(
    definitions: &TypeDefinitionTable,
    id: FirValueId,
    ty: &Ty,
    variant_name: Option<&str>,
    fields: &[(String, FirValueId)],
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let Ty::Nominal(owner) = ty else {
        return Err(shape("make-aggregate result is not nominal"));
    };
    let definition = definitions
        .get(owner)
        .ok_or_else(|| shape(format!("missing definition {owner:?}")))?;
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    let result = new_aggregate(ty, layouts, types, cursor)?;

    match &definition.kind {
        TypeDefinitionKind::Struct {
            fields: source_fields,
        } if variant_name.is_none() => {
            for (name, value) in fields {
                let source = source_fields
                    .iter()
                    .find(|field| &field.name == name)
                    .ok_or_else(|| shape(format!("unknown struct field `{name}`")))?;
                let placed = layout
                    .fields
                    .iter()
                    .find(|field| field.declaration_index == source.declaration_index)
                    .ok_or_else(|| shape("struct field missing from layout"))?;
                store_typed_value(
                    *value,
                    &source.ty,
                    add_offset(result.address, placed.offset, cursor)?,
                    stack_flags,
                    stack_flags,
                    scalars,
                    aggregates,
                    layouts,
                    cursor,
                )?;
            }
        }
        TypeDefinitionKind::Tagged { variants } => {
            let name = variant_name.ok_or_else(|| shape("tagged aggregate missing variant"))?;
            let variant = variants
                .iter()
                .find(|variant| variant.name == name)
                .ok_or_else(|| shape(format!("unknown variant `{name}`")))?;
            let LayoutKind::Tagged {
                variants: placed_variants,
                ..
            } = &layout.kind
            else {
                return Err(shape("tagged type has non-tagged layout"));
            };
            let placed_variant = placed_variants
                .iter()
                .find(|placed| placed.declaration_index == variant.declaration_index)
                .ok_or_else(|| shape("tagged variant missing from layout"))?;
            for (name, value) in fields {
                let source = variant
                    .fields
                    .iter()
                    .find(|field| &field.name == name)
                    .ok_or_else(|| shape(format!("unknown tagged field `{name}`")))?;
                let placed = placed_variant
                    .fields
                    .iter()
                    .find(|field| field.declaration_index == source.declaration_index)
                    .ok_or_else(|| shape("tagged field missing from layout"))?;
                store_typed_value(
                    *value,
                    &source.ty,
                    add_offset(result.address, placed.offset, cursor)?,
                    stack_flags,
                    stack_flags,
                    scalars,
                    aggregates,
                    layouts,
                    cursor,
                )?;
            }
            write_variant(
                &layout,
                variant.declaration_index,
                result.address,
                stack_flags,
                cursor,
            )?;
        }
        _ => return Err(shape("make-aggregate does not match nominal definition")),
    }
    aggregates.insert(id, result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_make_some(
    fir: &FirFunction,
    id: FirValueId,
    ty: &Ty,
    value: FirValueId,
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let Ty::Optional { inner } = ty else {
        return Err(shape("make-some result is not optional"));
    };
    if value_type(fir, value)? != inner.as_ref() {
        return Err(shape("make-some payload type mismatch"));
    }
    let result = new_aggregate(ty, layouts, types, cursor)?;
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    store_typed_value(
        value,
        inner,
        payload_address(&layout, result.address, cursor)?,
        stack_flags,
        stack_flags,
        scalars,
        aggregates,
        layouts,
        cursor,
    )?;
    write_variant(&layout, 1, result.address, stack_flags, cursor)?;
    aggregates.insert(id, result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_make_result_err(
    fir: &FirFunction,
    id: FirValueId,
    ty: &Ty,
    error: FirValueId,
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    lower_make_result_payload(
        fir,
        id,
        ty,
        error,
        false,
        stack_flags,
        scalars,
        aggregates,
        layouts,
        types,
        cursor,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_make_result_payload(
    fir: &FirFunction,
    id: FirValueId,
    ty: &Ty,
    payload: FirValueId,
    ok: bool,
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let Ty::Result { ok: ok_ty, error } = ty else {
        return Err(shape("result constructor result is not Result"));
    };
    let payload_ty = if ok { ok_ty.as_ref() } else { error.as_ref() };
    let variant = if ok { 0 } else { 1 };
    if value_type(fir, payload)? != payload_ty {
        return Err(shape("Result constructor payload type mismatch"));
    }
    let result = new_aggregate(ty, layouts, types, cursor)?;
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    if layouts.layout_of(payload_ty).map_err(layout_error)?.size != 0 {
        store_typed_value(
            payload,
            payload_ty,
            payload_address_for_variant(&layout, variant, result.address, cursor)?,
            stack_flags,
            stack_flags,
            scalars,
            aggregates,
            layouts,
            cursor,
        )?;
    }
    write_variant(&layout, variant, result.address, stack_flags, cursor)?;
    aggregates.insert(id, result);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn materialize_result(
    id: FirValueId,
    ty: &Ty,
    address: Value,
    source_flags: MemFlags,
    stack_flags: MemFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    // `void` occupies no storage and has no CLIF value representation. It can
    // occur when a payloadless Option/Result pattern projects `_`; preserving
    // that projection as a no-op keeps the aggregate's discriminant path real
    // without attempting to materialize a nonexistent scalar.
    if *ty == Ty::Void {
        return Ok(());
    }
    if is_memory_value(ty) {
        let result = new_aggregate(ty, layouts, types, cursor)?;
        let size = layouts.layout_of(ty).map_err(layout_error)?.size;
        copy_bytes(
            address,
            source_flags,
            result.address,
            stack_flags,
            size,
            cursor,
        )?;
        aggregates.insert(id, result);
    } else {
        let value = cursor
            .ins()
            .load(types.value_type(ty)?, source_flags, address, 0);
        scalars.insert(id, value);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn store_typed_value(
    id: FirValueId,
    ty: &Ty,
    address: Value,
    destination_flags: MemFlags,
    stack_flags: MemFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if *ty == Ty::Void {
        return Ok(());
    }
    if is_memory_value(ty) {
        let source = aggregate(aggregates, id)?;
        if &source.ty != ty {
            return Err(shape("aggregate store source type mismatch"));
        }
        let size = layouts.layout_of(ty).map_err(layout_error)?.size;
        copy_bytes(
            source.address,
            stack_flags,
            address,
            destination_flags,
            size,
            cursor,
        )
    } else {
        cursor
            .ins()
            .store(destination_flags, scalar(scalars, id)?, address, 0);
        Ok(())
    }
}

fn new_aggregate(
    ty: &Ty,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<AggregateValue, BackendError> {
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    let size = u32::try_from(layout.size.max(1))
        .map_err(|_| shape("aggregate temporary exceeds CLIF stack-slot size"))?;
    let slot = cursor.func.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        size,
        align_shift(layout.align)?,
    ));
    Ok(AggregateValue {
        address: cursor.ins().stack_addr(types.pointer_type()?, slot, 0),
        ty: ty.clone(),
    })
}

fn copy_bytes(
    source: Value,
    source_flags: MemFlags,
    destination: Value,
    destination_flags: MemFlags,
    size: u64,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    for offset in 0..size {
        let offset = i32_offset(offset)?;
        let byte = cursor
            .ins()
            .load(clif_types::I8, source_flags, source, offset);
        cursor
            .ins()
            .store(destination_flags, byte, destination, offset);
    }
    Ok(())
}

fn lower_len(
    ty: &Ty,
    address: Value,
    flags: MemFlags,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    match ty {
        Ty::Array {
            length: Some(length),
            ..
        } => {
            let length = i64::try_from(*length).map_err(|_| shape("array length exceeds i64"))?;
            Ok(cursor.ins().iconst(types.pointer_type()?, length))
        }
        Ty::Slice { .. } => {
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            let LayoutKind::Slice { len_offset, .. } = layout.kind else {
                return Err(shape("slice has non-slice layout"));
            };
            Ok(cursor.ins().load(
                types.pointer_type()?,
                flags,
                address,
                i32_offset(len_offset)?,
            ))
        }
        Ty::Str => {
            let layout = layouts.layout_of(ty).map_err(layout_error)?;
            let LayoutKind::Str { len_offset, .. } = layout.kind else {
                return Err(shape("str has non-str layout"));
            };
            Ok(cursor.ins().load(
                types.pointer_type()?,
                flags,
                address,
                i32_offset(len_offset)?,
            ))
        }
        _ => Err(shape(format!("len on unsupported type {ty:?}"))),
    }
}

fn payload_address(
    layout: &Layout,
    base: Value,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let offset = match sum_encoding(layout)? {
        SumEncoding::Tagged { payload_offset, .. } => *payload_offset,
        SumEncoding::Niche { .. } | SumEncoding::Single => 0,
    };
    add_offset(base, offset, cursor)
}

fn payload_address_for_variant(
    layout: &Layout,
    variant: u32,
    base: Value,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    match sum_encoding(layout)? {
        SumEncoding::Niche {
            payload_variant, ..
        } if *payload_variant != variant => Ok(base),
        _ => payload_address(layout, base, cursor),
    }
}

fn sum_encoding(layout: &Layout) -> Result<&SumEncoding, BackendError> {
    match &layout.kind {
        LayoutKind::Optional { encoding }
        | LayoutKind::Result { encoding }
        | LayoutKind::Tagged { encoding, .. } => Ok(encoding),
        _ => Err(shape("sum encoding requested from non-sum layout")),
    }
}

fn write_variant(
    layout: &Layout,
    variant: u32,
    base: Value,
    flags: MemFlags,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    match &layout.kind {
        LayoutKind::Enum { tag } => {
            if let Some(tag) = tag {
                store_integer_immediate(
                    tag.size as u16 * 8,
                    variant as u128,
                    base,
                    tag.offset,
                    flags,
                    cursor,
                )?;
            }
        }
        LayoutKind::Optional { encoding }
        | LayoutKind::Result { encoding }
        | LayoutKind::Tagged { encoding, .. } => match encoding {
            SumEncoding::Single => {}
            SumEncoding::Tagged { tag, .. } => {
                store_integer_immediate(
                    tag.size as u16 * 8,
                    variant as u128,
                    base,
                    tag.offset,
                    flags,
                    cursor,
                )?;
            }
            SumEncoding::Niche {
                payload_variant,
                niche_offset,
                niche_bits,
                fieldless_values,
            } => {
                if *payload_variant != variant {
                    let value = fieldless_values
                        .iter()
                        .find(|(index, _)| *index == variant)
                        .map(|(_, value)| *value)
                        .ok_or_else(|| shape("missing niche for fieldless variant"))?;
                    store_integer_immediate(
                        *niche_bits as u16,
                        value,
                        base,
                        *niche_offset,
                        flags,
                        cursor,
                    )?;
                }
            }
        },
        _ => return Err(shape("variant write on non-sum layout")),
    }
    Ok(())
}

fn read_variant_test(
    layout: &Layout,
    variant: u32,
    base: Value,
    flags: MemFlags,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    match &layout.kind {
        LayoutKind::Enum { tag: Some(tag) } => {
            let raw = load_integer(tag.size as u16 * 8, base, tag.offset, flags, cursor)?;
            Ok(cursor.ins().icmp_imm_u(IntCC::Equal, raw, variant as i64))
        }
        LayoutKind::Enum { tag: None } => Ok(cursor.ins().iconst(clif_types::I8, 1)),
        LayoutKind::Optional { encoding }
        | LayoutKind::Result { encoding }
        | LayoutKind::Tagged { encoding, .. } => match encoding {
            SumEncoding::Single => Ok(cursor.ins().iconst(clif_types::I8, 1)),
            SumEncoding::Tagged { tag, .. } => {
                let raw = load_integer(tag.size as u16 * 8, base, tag.offset, flags, cursor)?;
                Ok(cursor.ins().icmp_imm_u(IntCC::Equal, raw, variant as i64))
            }
            SumEncoding::Niche {
                payload_variant,
                niche_offset,
                niche_bits,
                fieldless_values,
            } => {
                let raw = load_integer(*niche_bits as u16, base, *niche_offset, flags, cursor)?;
                if *payload_variant == variant {
                    let mut result = cursor.ins().iconst(clif_types::I8, 1);
                    for (_, niche) in fieldless_values {
                        let valid = cursor.ins().icmp_imm_u(IntCC::NotEqual, raw, *niche as i64);
                        result = cursor.ins().band(result, valid);
                    }
                    Ok(result)
                } else {
                    let niche = fieldless_values
                        .iter()
                        .find(|(index, _)| *index == variant)
                        .map(|(_, value)| *value)
                        .ok_or_else(|| shape("missing niche for tested variant"))?;
                    Ok(cursor.ins().icmp_imm_u(IntCC::Equal, raw, niche as i64))
                }
            }
        },
        _ => Err(shape("variant test on non-sum layout")),
    }
}

fn variant_index(
    definitions: &TypeDefinitionTable,
    ty: &Ty,
    name: &str,
) -> Result<u32, BackendError> {
    let Ty::Nominal(owner) = ty else {
        return Err(shape("named variant on non-nominal type"));
    };
    let definition = definitions
        .get(owner)
        .ok_or_else(|| shape(format!("missing definition {owner:?}")))?;
    match &definition.kind {
        TypeDefinitionKind::Enum { variants } | TypeDefinitionKind::Tagged { variants } => variants
            .iter()
            .find(|variant| variant.name == name)
            .map(|variant| variant.declaration_index)
            .ok_or_else(|| shape(format!("unknown variant `{name}`"))),
        _ => Err(shape("named variant on non-enum/tagged type")),
    }
}

fn store_integer_immediate(
    bits: u16,
    value: u128,
    base: Value,
    offset: u64,
    flags: MemFlags,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let value = cursor
        .ins()
        .iconst(clif_integer_type(bits)?, value as u64 as i64);
    cursor.ins().store(flags, value, base, i32_offset(offset)?);
    Ok(())
}

fn load_integer(
    bits: u16,
    base: Value,
    offset: u64,
    flags: MemFlags,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    Ok(cursor
        .ins()
        .load(clif_integer_type(bits)?, flags, base, i32_offset(offset)?))
}

fn clif_integer_type(bits: u16) -> Result<cranelift_codegen::ir::Type, BackendError> {
    match bits {
        8 => Ok(clif_types::I8),
        16 => Ok(clif_types::I16),
        32 => Ok(clif_types::I32),
        64 => Ok(clif_types::I64),
        _ => Err(shape(format!("unsupported stored integer width {bits}"))),
    }
}

fn normalize_index(
    ty: &Ty,
    value: Value,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let source = types.value_type(ty)?;
    let target = types.pointer_type()?;
    Ok(match source.bits().cmp(&target.bits()) {
        std::cmp::Ordering::Equal => value,
        std::cmp::Ordering::Greater => cursor.ins().ireduce(target, value),
        std::cmp::Ordering::Less if matches!(ty, Ty::Int { signed: true, .. }) => {
            cursor.ins().sextend(target, value)
        }
        std::cmp::Ordering::Less => cursor.ins().uextend(target, value),
    })
}

fn scale_index(
    index: Value,
    stride: u64,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    if stride == 0 {
        return Ok(cursor.ins().iconst(types.pointer_type()?, 0));
    }
    if stride == 1 {
        return Ok(index);
    }
    let stride = i64::try_from(stride).map_err(|_| shape("element stride exceeds i64"))?;
    let scale = cursor.ins().iconst(types.pointer_type()?, stride);
    Ok(cursor.ins().imul(index, scale))
}

fn add_offset(
    base: Value,
    offset: u64,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    if offset == 0 {
        return Ok(base);
    }
    let offset = i64::try_from(offset).map_err(|_| shape("layout offset exceeds i64"))?;
    Ok(cursor.ins().iadd_imm_u(base, offset))
}

fn value_type(fir: &FirFunction, id: FirValueId) -> Result<&Ty, BackendError> {
    fir.value_types
        .get(&id)
        .ok_or_else(|| shape(format!("missing type for FIR value {id:?}")))
}

fn scalar(values: &BTreeMap<FirValueId, Value>, id: FirValueId) -> Result<Value, BackendError> {
    values
        .get(&id)
        .copied()
        .ok_or_else(|| shape(format!("scalar FIR value {id:?} used before lowering")))
}

fn aggregate(
    values: &BTreeMap<FirValueId, AggregateValue>,
    id: FirValueId,
) -> Result<&AggregateValue, BackendError> {
    values
        .get(&id)
        .ok_or_else(|| shape(format!("aggregate FIR value {id:?} used before lowering")))
}

fn align_shift(align: u64) -> Result<u8, BackendError> {
    if !align.is_power_of_two() {
        return Err(shape("layout alignment is not power-of-two"));
    }
    u8::try_from(align.trailing_zeros()).map_err(|_| shape("alignment shift exceeds u8"))
}

fn i32_offset(offset: u64) -> Result<i32, BackendError> {
    i32::try_from(offset).map_err(|_| shape("memory offset exceeds CLIF i32 offset"))
}

fn layout_error(error: forge_fir::LayoutError) -> BackendError {
    BackendError::InvalidFirShape {
        message: format!("Forge layout failure during C9c lowering: {error}"),
    }
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
