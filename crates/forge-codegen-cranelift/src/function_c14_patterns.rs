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
    if matches!(&instruction.kind, FirInstructionKind::Unit) {
        if let Some(result) = instruction.result {
            if value_type(fir, result)? != &Ty::Void {
                return Err(shape("unit result is not void typed"));
            }
        }
        return Ok(());
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
            key,
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

    lower_c14_duration_instruction(
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

#[allow(clippy::too_many_arguments)]
fn lower_c14_collection_pattern_lookup(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    collection: FirValueId,
    operation: DefId,
    key: &str,
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee = all_functions.get(&operation).ok_or_else(|| {
        shape(format!(
            "collection-pattern lookup target {operation:?} is not in the module"
        ))
    })?;
    let params = c14_function_parameter_types(callee)?;
    if params.len() != 2 || params[1] != Ty::Str {
        return Err(shape(
            "collection-pattern lookup target does not have (self, str) parameters",
        ));
    }

    let (self_id, key_id) = c14_synthetic_argument_ids(fir)?;
    let mut caller = fir.clone();
    caller.value_types.insert(self_id, params[0].clone());
    caller.value_types.insert(key_id, Ty::Str);

    let self_value = c14_collection_self_value(
        fir,
        collection,
        &params[0],
        scalars,
        aggregates,
    )?;
    scalars.insert(self_id, self_value);
    let key_value = c14_stack_str(key, flags, types, layouts, cursor)?;
    aggregates.insert(key_id, key_value);

    let call = FirInstruction {
        span: instruction.span,
        result: instruction.result,
        kind: FirInstructionKind::Call {
            target: operation,
            args: vec![self_id, key_id],
            tail: false,
        },
    };
    let result = lower_c9d_direct_call(
        &caller,
        all_functions,
        definitions,
        &call,
        operation,
        &[self_id, key_id],
        call_conv,
        direct_functions,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    );
    scalars.remove(&self_id);
    aggregates.remove(&key_id);
    result
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_collection_pattern_has_only(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    collection: FirValueId,
    operation: DefId,
    keys: &[String],
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee = all_functions.get(&operation).ok_or_else(|| {
        shape(format!(
            "collection-pattern has-only target {operation:?} is not in the module"
        ))
    })?;
    let params = c14_function_parameter_types(callee)?;
    let expected_keys = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Str),
    };
    if params.len() != 2 || params[1] != expected_keys {
        return Err(shape(
            "collection-pattern has-only target does not have (self, str[]) parameters",
        ));
    }

    let (self_id, keys_id) = c14_synthetic_argument_ids(fir)?;
    let mut caller = fir.clone();
    caller.value_types.insert(self_id, params[0].clone());
    caller.value_types.insert(keys_id, expected_keys.clone());

    let self_value = c14_collection_self_value(
        fir,
        collection,
        &params[0],
        scalars,
        aggregates,
    )?;
    scalars.insert(self_id, self_value);
    let keys_value = c14_stack_str_slice(keys, flags, types, layouts, cursor)?;
    aggregates.insert(keys_id, keys_value);

    let call = FirInstruction {
        span: instruction.span,
        result: instruction.result,
        kind: FirInstructionKind::Call {
            target: operation,
            args: vec![self_id, keys_id],
            tail: false,
        },
    };
    let result = lower_c9d_direct_call(
        &caller,
        all_functions,
        definitions,
        &call,
        operation,
        &[self_id, keys_id],
        call_conv,
        direct_functions,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    );
    scalars.remove(&self_id);
    aggregates.remove(&keys_id);
    result
}

fn c14_function_parameter_types(function: &FirFunction) -> Result<Vec<Ty>, BackendError> {
    function
        .params
        .iter()
        .map(|id| {
            function
                .locals
                .get(id)
                .map(|local| local.ty.clone())
                .ok_or_else(|| shape(format!("missing collection protocol parameter {id:?}")))
        })
        .collect()
}

fn c14_synthetic_argument_ids(
    fir: &FirFunction,
) -> Result<(FirValueId, FirValueId), BackendError> {
    let mut candidate = u32::MAX;
    let mut found = Vec::with_capacity(2);
    while found.len() < 2 {
        let id = FirValueId(candidate);
        if !fir.value_types.contains_key(&id) {
            found.push(id);
        }
        if candidate == 0 && found.len() < 2 {
            return Err(shape("no FIR value ids remain for collection-pattern arguments"));
        }
        candidate = candidate.saturating_sub(1);
    }
    Ok((found[0], found[1]))
}

fn c14_collection_self_value(
    fir: &FirFunction,
    collection: FirValueId,
    expected: &Ty,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
) -> Result<Value, BackendError> {
    let source_ty = value_type(fir, collection)?;
    if source_ty == expected {
        return scalar(scalars, collection);
    }
    match expected {
        Ty::Reference {
            mutable: false,
            inner,
        } if inner.as_ref() == source_ty => Ok(aggregate(aggregates, collection)?.address),
        _ => Err(shape(format!(
            "collection-pattern self type mismatch: source {source_ty:?}, target {expected:?}"
        ))),
    }
}

fn c14_stack_str(
    text: &str,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<AggregateValue, BackendError> {
    let byte_count = u32::try_from(text.len().max(1))
        .map_err(|_| shape("collection-pattern key exceeds stack-slot size"))?;
    let bytes = cursor.func.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        byte_count,
        0,
    ));
    let data = cursor.ins().stack_addr(types.pointer_type()?, bytes, 0);
    for (index, byte) in text.bytes().enumerate() {
        let value = cursor.ins().iconst(clif_types::I8, i64::from(byte));
        let offset = i32::try_from(index)
            .map_err(|_| shape("collection-pattern key offset exceeds i32"))?;
        cursor.ins().store(flags.stack, value, data, offset);
    }

    let result = new_aggregate(&Ty::Str, layouts, types, cursor)?;
    let layout = layouts.layout_of(&Ty::Str).map_err(layout_error)?;
    let LayoutKind::Str {
        data_offset,
        len_offset,
    } = layout.kind
    else {
        return Err(shape("str has non-str layout"));
    };
    let len = i64::try_from(text.len())
        .map_err(|_| shape("collection-pattern key length exceeds i64"))?;
    let len = cursor.ins().iconst(types.pointer_type()?, len);
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
    Ok(result)
}

fn c14_stack_str_slice(
    keys: &[String],
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<AggregateValue, BackendError> {
    let mut values = Vec::with_capacity(keys.len());
    for key in keys {
        values.push(c14_stack_str(key, flags, types, layouts, cursor)?);
    }

    let str_layout = layouts.layout_of(&Ty::Str).map_err(layout_error)?;
    let bytes = str_layout
        .size
        .checked_mul(keys.len() as u64)
        .ok_or_else(|| shape("collection-pattern key slice size overflow"))?;
    let slot_size = u32::try_from(bytes.max(1))
        .map_err(|_| shape("collection-pattern key slice exceeds stack-slot size"))?;
    let array = cursor.func.create_sized_stack_slot(StackSlotData::new(
        StackSlotKind::ExplicitSlot,
        slot_size,
        align_shift(str_layout.align)?,
    ));
    let data = cursor.ins().stack_addr(types.pointer_type()?, array, 0);
    for (index, value) in values.iter().enumerate() {
        let offset = str_layout
            .size
            .checked_mul(index as u64)
            .ok_or_else(|| shape("collection-pattern key slice offset overflow"))?;
        let destination = add_offset(data, offset, cursor)?;
        copy_bytes(
            value.address,
            flags.stack,
            destination,
            flags.stack,
            str_layout.size,
            cursor,
        )?;
    }

    let slice_ty = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Str),
    };
    let result = new_aggregate(&slice_ty, layouts, types, cursor)?;
    let layout = layouts.layout_of(&slice_ty).map_err(layout_error)?;
    let LayoutKind::Slice {
        data_offset,
        len_offset,
    } = layout.kind
    else {
        return Err(shape("str[] has non-slice layout"));
    };
    let len = i64::try_from(keys.len())
        .map_err(|_| shape("collection-pattern key count exceeds i64"))?;
    let len = cursor.ins().iconst(types.pointer_type()?, len);
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
    Ok(result)
}
