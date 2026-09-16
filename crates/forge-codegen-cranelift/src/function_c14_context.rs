#[allow(clippy::too_many_arguments)]
fn lower_c14_context_instruction(
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
    match &instruction.kind {
        FirInstructionKind::ContextLoad { slot } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("context load has no result"))?;
            if !is_context_value_type(value_type(fir, result)?) {
                return Err(shape("context load result is not a non-owning pointer/reference"));
            }
            let address = c14_context_slot_address(all_globals, *slot, types, cursor)?;
            let value = cursor
                .ins()
                .load(types.pointer_type()?, flags.deref, address, 0);
            scalars.insert(result, value);
            return Ok(());
        }
        FirInstructionKind::ContextSave { slot } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("context save has no result"))?;
            if value_type(fir, result)? != &Ty::ContextSlot { slot: *slot } {
                return Err(shape("context save result does not match its slot"));
            }
            let address = c14_context_slot_address(all_globals, *slot, types, cursor)?;
            let value = cursor
                .ins()
                .load(types.pointer_type()?, flags.deref, address, 0);
            scalars.insert(result, value);
            return Ok(());
        }
        FirInstructionKind::ContextSet { slot, value } => {
            if instruction.result.is_some() {
                return Err(shape("context set unexpectedly has a result"));
            }
            if !is_context_value_type(value_type(fir, *value)?) {
                return Err(shape("context set value is not a non-owning pointer/reference"));
            }
            let address = c14_context_slot_address(all_globals, *slot, types, cursor)?;
            cursor
                .ins()
                .store(flags.deref, scalar(scalars, *value)?, address, 0);
            return Ok(());
        }
        FirInstructionKind::ContextRestore { slot, saved } => {
            if instruction.result.is_some() {
                return Err(shape("context restore unexpectedly has a result"));
            }
            if value_type(fir, *saved)? != &Ty::ContextSlot { slot: *slot } {
                return Err(shape("context restore value does not match its slot"));
            }
            let address = c14_context_slot_address(all_globals, *slot, types, cursor)?;
            cursor
                .ins()
                .store(flags.deref, scalar(scalars, *saved)?, address, 0);
            return Ok(());
        }
        _ => {}
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

fn c14_context_slot_address(
    all_globals: &BTreeMap<DefId, FirGlobal>,
    slot: ContextSlot,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let global = crate::context::storage_owner(all_globals, slot)?;
    let name = cursor
        .func
        .declare_imported_user_function(UserExternalName::new(1, global.0));
    let symbolic = cursor.func.create_global_value(GlobalValueData::Symbol {
        name: ExternalName::user(name),
        offset: 0.into(),
        colocated: true,
        tls: false,
    });
    Ok(cursor.ins().symbol_value(types.pointer_type()?, symbolic))
}

fn is_context_value_type(ty: &Ty) -> bool {
    matches!(ty, Ty::Pointer { .. } | Ty::Reference { .. })
}
