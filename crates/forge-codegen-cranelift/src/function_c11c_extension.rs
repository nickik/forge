use cranelift_codegen::ir::GlobalValueData;
use forge_fir::FirGlobal;

/// C11c keeps the C9d call/aggregate ABI intact and only extends instruction
/// lowering with symbolic global reads.
pub(crate) fn lower_function_c11c(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if !fir.closures.is_empty() {
        return Err(BackendError::UnsupportedFir {
            component: "closures",
        });
    }

    let call_conv = CallConv::triple_default(isa.triple());
    let signature_plan = lower_c9_fir_signature(fir, definitions, types, call_conv)?;
    let entry_types = signature_plan
        .signature
        .params
        .iter()
        .map(|param| param.value_type)
        .collect::<Vec<_>>();
    let mut function = Function::with_name_signature(
        UserFuncName::user(0, fir.owner.0),
        signature_plan.signature.clone(),
    );
    let mut layouts = LayoutEngine::new(LayoutTarget::new(types.target().pointer_bits), definitions);
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
    let entry_values = entry_types
        .into_iter()
        .map(|ty| function.dfg.append_block_param(entry, ty))
        .collect::<Vec<_>>();

    let mut scalars = BTreeMap::<FirValueId, Value>::new();
    let mut aggregates = BTreeMap::<FirValueId, AggregateValue>::new();
    let mut direct_functions = BTreeMap::<DefId, FuncRef>::new();

    let hidden_return = initialize_c9d_parameters(
        fir,
        &signature_plan,
        &entry_values,
        &local_slots,
        flags,
        types,
        &mut layouts,
        &mut function,
    )?;

    for fir_block in &fir.blocks {
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing CLIF block for {:?}", fir_block.id)))?;
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(clif_block);
        for instruction in &fir_block.instructions {
            lower_c11c_instruction(
                fir,
                all_functions,
                all_globals,
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
        lower_c9d_terminator(
            fir,
            terminator,
            &signature_plan.result,
            hidden_return,
            &blocks,
            flags,
            &scalars,
            &aggregates,
            types,
            &mut layouts,
            &mut cursor,
        )?;
    }

    verify_function(&function, isa).map_err(|errors| BackendError::Cranelift {
        message: format!("CLIF verifier rejected C11c FIR function {:?}: {errors}", fir.owner),
    })?;
    Ok(function)
}

#[allow(clippy::too_many_arguments)]
fn lower_c11c_instruction(
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
    if let FirInstructionKind::LoadGlobal { global } = &instruction.kind {
        return lower_c11c_load_global(
            fir,
            all_globals,
            instruction,
            *global,
            flags,
            scalars,
            aggregates,
            types,
            layouts,
            cursor,
        );
    }

    lower_c9d_instruction(
        fir,
        all_functions,
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
fn lower_c11c_load_global(
    fir: &FirFunction,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    instruction: &FirInstruction,
    global: DefId,
    flags: MemoryFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result = instruction
        .result
        .ok_or_else(|| shape("global load has no FIR result"))?;
    let result_ty = value_type(fir, result)?;
    let global_data = all_globals
        .get(&global)
        .ok_or_else(|| shape(format!("global load refers to missing global {global:?}")))?;
    if result_ty != &global_data.ty {
        return Err(shape(format!(
            "global load {global:?} result type {result_ty:?} differs from global type {:?}",
            global_data.ty
        )));
    }

    // Namespace 1 is reserved by the Forge Cranelift boundary for data
    // definitions. Namespace 0 remains the existing C9/C10 function namespace.
    let name = cursor
        .func
        .declare_imported_user_function(UserExternalName::new(1, global.0));
    let symbolic = cursor.func.create_global_value(GlobalValueData::Symbol {
        name: ExternalName::user(name),
        offset: 0.into(),
        colocated: true,
        tls: false,
    });
    let address = cursor.ins().symbol_value(types.pointer_type()?, symbolic);

    if is_memory_value(result_ty) {
        materialize_result(
            result,
            result_ty,
            address,
            flags.deref,
            flags.stack,
            scalars,
            aggregates,
            layouts,
            types,
            cursor,
        )?;
    } else {
        let value = cursor
            .ins()
            .load(types.value_type(result_ty)?, flags.deref, address, 0);
        scalars.insert(result, value);
    }
    Ok(())
}
