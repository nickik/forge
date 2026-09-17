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
    if let FirInstructionKind::CollectionPatternLookup {
        collection,
        operation,
        key,
    } = &instruction.kind
    {
        return lower_c14_collection_pattern_lookup(
            fir,
            all_functions,
            definitions,
            instruction,
            *collection,
            *operation,
            *key,
            flags,
            call_conv,
            direct_functions,
            scalars,
            aggregates,
            types,
            layouts,
            cursor,
        );
    }

    if let FirInstructionKind::CollectionPatternHasOnly {
        collection,
        operation,
        keys,
    } = &instruction.kind
    {
        return lower_c14_collection_pattern_has_only(
            fir,
            all_functions,
            definitions,
            instruction,
            *collection,
            *operation,
            keys,
            flags,
            call_conv,
            direct_functions,
            scalars,
            aggregates,
            types,
            layouts,
            cursor,
        );
    }

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
fn lower_c14_collection_pattern_lookup(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    collection: FirValueId,
    operation: DefId,
    key: FirValueId,
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if value_type(fir, key)? != &Ty::Str {
        return Err(shape("collection-pattern lookup key is not str"));
    }
    let (receiver_ty, receiver) =
        collection_pattern_receiver(fir, collection, scalars, aggregates)?;
    let receiver_id = fresh_pattern_value(fir, 0)?;
    let mut caller = fir.clone();
    caller.value_types.insert(receiver_id, receiver_ty);
    scalars.insert(receiver_id, receiver);
    let call = FirInstruction {
        span: instruction.span,
        result: instruction.result,
        kind: FirInstructionKind::Call {
            target: operation,
            args: vec![receiver_id, key],
            tail: false,
        },
    };
    lower_c9d_direct_call(
        &caller,
        all_functions,
        definitions,
        &call,
        operation,
        &[receiver_id, key],
        call_conv,
        direct_functions,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_collection_pattern_has_only(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    collection: FirValueId,
    operation: DefId,
    keys: &[FirValueId],
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let (receiver_ty, receiver) =
        collection_pattern_receiver(fir, collection, scalars, aggregates)?;
    let receiver_id = fresh_pattern_value(fir, 0)?;
    let keys_id = fresh_pattern_value(fir, 1)?;
    let keys_ty = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Str),
    };
    let keys_value =
        materialize_pattern_keys(keys, fir, flags, aggregates, types, layouts, cursor)?;

    let mut caller = fir.clone();
    caller.value_types.insert(receiver_id, receiver_ty);
    caller.value_types.insert(keys_id, keys_ty);
    scalars.insert(receiver_id, receiver);
    aggregates.insert(keys_id, keys_value);
    let call = FirInstruction {
        span: instruction.span,
        result: instruction.result,
        kind: FirInstructionKind::Call {
            target: operation,
            args: vec![receiver_id, keys_id],
            tail: false,
        },
    };
    lower_c9d_direct_call(
        &caller,
        all_functions,
        definitions,
        &call,
        operation,
        &[receiver_id, keys_id],
        call_conv,
        direct_functions,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )
}

fn collection_pattern_receiver(
    fir: &FirFunction,
    collection: FirValueId,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
) -> Result<(Ty, Value), BackendError> {
    match value_type(fir, collection)? {
        ty @ Ty::Reference { mutable: false, .. } => Ok((ty.clone(), scalar(scalars, collection)?)),
        ty @ Ty::Nominal(_) => Ok((
            Ty::Reference {
                mutable: false,
                inner: Box::new(ty.clone()),
            },
            aggregate(aggregates, collection)?.address,
        )),
        _ => Err(shape(
            "collection-pattern receiver is neither a nominal value nor immutable reference",
        )),
    }
}

fn fresh_pattern_value(fir: &FirFunction, offset: u32) -> Result<FirValueId, BackendError> {
    let next = match fir.value_types.keys().next_back() {
        Some(id) => {
            id.0.checked_add(1)
                .ok_or_else(|| shape("no FIR value id remains for collection-pattern lowering"))?
        }
        None => 0,
    };
    next.checked_add(offset)
        .map(FirValueId)
        .ok_or_else(|| shape("no FIR value id remains for collection-pattern lowering"))
}

fn materialize_pattern_keys(
    keys: &[FirValueId],
    fir: &FirFunction,
    flags: MemoryFlags,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<AggregateValue, BackendError> {
    let length =
        u64::try_from(keys.len()).map_err(|_| shape("too many collection-pattern keys"))?;
    let array_ty = Ty::Array {
        element: Box::new(Ty::Str),
        length: Some(length),
    };
    let array = new_aggregate(&array_ty, layouts, types, cursor)?;
    let array_layout = layouts.layout_of(&array_ty).map_err(layout_error)?;
    let LayoutKind::Array { stride, .. } = array_layout.kind else {
        return Err(shape("collection-pattern key array has non-array layout"));
    };
    let str_size = layouts.layout_of(&Ty::Str).map_err(layout_error)?.size;
    for (index, key) in keys.iter().enumerate() {
        if value_type(fir, *key)? != &Ty::Str {
            return Err(shape("collection-pattern key is not str"));
        }
        let offset = stride
            .checked_mul(index as u64)
            .ok_or_else(|| shape("collection-pattern key array offset overflow"))?;
        copy_bytes(
            aggregate(aggregates, *key)?.address,
            flags.stack,
            add_offset(array.address, offset, cursor)?,
            flags.stack,
            str_size,
            cursor,
        )?;
    }

    let slice_ty = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Str),
    };
    let slice = new_aggregate(&slice_ty, layouts, types, cursor)?;
    let slice_layout = layouts.layout_of(&slice_ty).map_err(layout_error)?;
    let LayoutKind::Slice {
        data_offset,
        len_offset,
    } = slice_layout.kind
    else {
        return Err(shape("collection-pattern keys have non-slice layout"));
    };
    cursor.ins().store(
        flags.stack,
        array.address,
        slice.address,
        i32_offset(data_offset)?,
    );
    let length = cursor.ins().iconst(
        types.pointer_type()?,
        i64::try_from(length).map_err(|_| shape("collection-pattern key count exceeds i64"))?,
    );
    cursor
        .ins()
        .store(flags.stack, length, slice.address, i32_offset(len_offset)?);
    Ok(slice)
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
            _ => {
                return Err(shape(
                    "slice conversion source is not a reference to a fixed array",
                ))
            }
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
        return Err(shape(
            "slice conversion source and result types are incompatible",
        ));
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
    cursor
        .ins()
        .store(flags.stack, length, output.address, i32_offset(len_offset)?);
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
            cursor
                .ins()
                .store(flags.stack, data, result.address, i32_offset(data_offset)?);
            cursor
                .ins()
                .store(flags.stack, len, result.address, i32_offset(len_offset)?);
            aggregates.insert(result_id, result);
            Ok(())
        }
        _ => Err(BackendError::UnsupportedInstruction {
            kind: "subsequence source/result type combination",
        }),
    }
}
