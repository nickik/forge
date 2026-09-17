#[allow(clippy::too_many_arguments)]
fn lower_c14_pattern_instruction(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
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
    if let FirInstructionKind::SliceFromArrayRef { value } = &instruction.kind {
        return lower_c14_slice_from_array_ref(
            fir,
            instruction,
            *value,
            flags,
            scalars,
            aggregates,
            types,
            layouts,
            cursor,
        );
    }

    if let FirInstructionKind::Subsequence { base, start } = &instruction.kind {
        return lower_c14_subsequence(
            fir,
            instruction,
            *base,
            *start,
            flags,
            scalars,
            aggregates,
            types,
            layouts,
            cursor,
        );
    }

    lower_c11c_instruction(
        fir,
        all_functions,
        all_globals,
        definitions,
        instruction,
        local_slots,
        flags,
        call_conv,
        direct_functions,
        scalars,
        aggregates,
        types,
        layouts,
        cursor,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_slice_from_array_ref(
    fir: &FirFunction,
    instruction: &FirInstruction,
    input: FirValueId,
    flags: MemoryFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result = instruction
        .result
        .ok_or_else(|| shape("slice-from-array reference has no result"))?;
    let input_ty = value_type(fir, input)?;
    let result_ty = value_type(fir, result)?.clone();
    let (reference_mutable, element, length) = match input_ty {
        Ty::Reference { mutable, inner } => match inner.as_ref() {
            Ty::Array {
                element,
                length: Some(length),
            } => (*mutable, element.as_ref(), *length),
            _ => return Err(shape("slice conversion source is not a reference to a fixed array")),
        },
        _ => return Err(shape("slice conversion source is not a reference")),
    };
    let Ty::Slice {
        mutable: result_mutable,
        element: result_element,
    } = &result_ty
    else {
        return Err(shape("slice-from-array reference result is not a slice"));
    };
    if element != result_element.as_ref() || (*result_mutable && !reference_mutable) {
        return Err(shape("slice conversion source and result types are incompatible"));
    }

    let layout = layouts.layout_of(&result_ty).map_err(layout_error)?;
    let LayoutKind::Slice {
        data_offset,
        len_offset,
        ..
    } = layout.kind
    else {
        return Err(shape("slice has non-slice layout"));
    };
    let output = new_aggregate(&result_ty, layouts, types, cursor)?;
    cursor.ins().store(
        flags.stack,
        scalar(scalars, input)?,
        output.address,
        i32_offset(data_offset)?,
    );
    let length = i64::try_from(length).map_err(|_| shape("array length exceeds i64"))?;
    let length = cursor.ins().iconst(types.pointer_type()?, length);
    cursor.ins().store(
        flags.stack,
        length,
        output.address,
        i32_offset(len_offset)?,
    );
    aggregates.insert(result, output);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_subsequence(
    fir: &FirFunction,
    instruction: &FirInstruction,
    base: FirValueId,
    start: u64,
    flags: MemoryFlags,
    _scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result_id = instruction
        .result
        .ok_or_else(|| shape("subsequence has no result"))?;
    let base_ty = value_type(fir, base)?.clone();
    let result_ty = value_type(fir, result_id)?.clone();
    let source_address = aggregate(aggregates, base)?.address;

    match (&base_ty, &result_ty) {
        (
            Ty::Array {
                element: base_element,
                length: Some(base_len),
            },
            Ty::Array {
                element: result_element,
                length: Some(result_len),
            },
        ) if base_element == result_element
            && start <= *base_len
            && *result_len == *base_len - start =>
        {
            let base_layout = layouts.layout_of(&base_ty).map_err(layout_error)?;
            let LayoutKind::Array { stride, .. } = base_layout.kind else {
                return Err(shape("array subsequence has non-array layout"));
            };
            let byte_offset = stride
                .checked_mul(start)
                .ok_or_else(|| shape("array subsequence offset overflow"))?;
            let source = add_offset(source_address, byte_offset, cursor)?;
            let result = new_aggregate(&result_ty, layouts, types, cursor)?;
            let size = layouts.layout_of(&result_ty).map_err(layout_error)?.size;
            copy_bytes(
                source,
                flags.stack,
                result.address,
                flags.stack,
                size,
                cursor,
            )?;
            aggregates.insert(result_id, result);
            Ok(())
        }
        (
            Ty::Slice {
                mutable: base_mutable,
                element: base_element,
            },
            Ty::Slice {
                mutable: result_mutable,
                element: result_element,
            },
        ) if base_mutable == result_mutable && base_element == result_element => {
            let base_layout = layouts.layout_of(&base_ty).map_err(layout_error)?;
            let LayoutKind::Slice {
                data_offset,
                len_offset,
                ..
            } = base_layout.kind
            else {
                return Err(shape("slice subsequence has non-slice layout"));
            };
            let mut data = cursor.ins().load(
                types.pointer_type()?,
                flags.stack,
                source_address,
                i32_offset(data_offset)?,
            );
            let len = cursor.ins().load(
                types.pointer_type()?,
                flags.stack,
                source_address,
                i32_offset(len_offset)?,
            );
            let stride = layouts.layout_of(base_element).map_err(layout_error)?.size;
            let byte_offset = stride
                .checked_mul(start)
                .ok_or_else(|| shape("slice subsequence offset overflow"))?;
            data = add_offset(data, byte_offset, cursor)?;
            let start =
                i64::try_from(start).map_err(|_| shape("slice subsequence start exceeds i64"))?;
            let start_value = cursor.ins().iconst(types.pointer_type()?, start);
            let len = cursor.ins().isub(len, start_value);

            let result = new_aggregate(&result_ty, layouts, types, cursor)?;
            cursor.ins().store(
                flags.stack,
                data,
                result.address,
                i32_offset(data_offset)?,
            );
            cursor.ins().store(
                flags.stack,
                len,
                result.address,
                i32_offset(len_offset)?,
            );
            aggregates.insert(result_id, result);
            Ok(())
        }
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "subsequence source/result type combination",
        }),
    }
}
