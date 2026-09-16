use forge_fir::FirConst;

/// C14 scalar completion for concrete scalar types that the older C4/C9
/// integer-only lowering did not understand.
///
/// Keep this layer above C11c/C9d so the frozen C9 ABI and aggregate machinery
/// remain unchanged. Captured closures continue through their dedicated C14
/// lowering; this function handles ordinary functions without local closures.
pub(crate) fn lower_function_c14_scalar(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    let has_closure_value = fir
        .locals
        .values()
        .any(|local| matches!(&local.ty, Ty::Closure { .. }));
    let has_closure_call = fir
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(&instruction.kind, FirInstructionKind::CallClosure { .. }));
    if !fir.closures.is_empty() || has_closure_value || has_closure_call {
        return lower_function_c14(fir, all_functions, all_globals, definitions, types, isa);
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
            lower_c14_scalar_instruction(
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
        message: format!(
            "CLIF verifier rejected C14 scalar FIR function {:?}: {errors}",
            fir.owner
        ),
    })?;
    Ok(function)
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_scalar_instruction(
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
        FirInstructionKind::Const {
            value: FirConst::Char { value },
        } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("char constant has no result"))?;
            if value_type(fir, result)? != &Ty::Char {
                return Err(shape("char constant result is not char typed"));
            }
            let value = cursor
                .ins()
                .iconst(clif_types::I32, i64::from(u32::from(*value)));
            scalars.insert(result, value);
            return Ok(());
        }
        FirInstructionKind::Load {
            place: FirPlace::Local { local },
        } if fir.locals.get(local).is_some_and(|local| local.ty == Ty::Char) => {
            let result = instruction
                .result
                .ok_or_else(|| shape("char local load has no result"))?;
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing char local stack slot {local:?}")))?;
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            let value = cursor.ins().load(clif_types::I32, flags.stack, address, 0);
            scalars.insert(result, value);
            return Ok(());
        }
        FirInstructionKind::Store {
            place: FirPlace::Local { local },
            value,
        } if fir.locals.get(local).is_some_and(|local| local.ty == Ty::Char) => {
            if instruction.result.is_some() {
                return Err(shape("char local store unexpectedly has a result"));
            }
            if value_type(fir, *value)? != &Ty::Char {
                return Err(shape("char local store value is not char typed"));
            }
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing char local stack slot {local:?}")))?;
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            cursor
                .ins()
                .store(flags.stack, scalar(scalars, *value)?, address, 0);
            return Ok(());
        }
        FirInstructionKind::Binary {
            op,
            left,
            right,
            ..
        } if value_type(fir, *left)? == &Ty::Char && value_type(fir, *right)? == &Ty::Char => {
            let cc = match op {
                forge_fir::BinaryOp::Eq => IntCC::Equal,
                forge_fir::BinaryOp::NotEq => IntCC::NotEqual,
                forge_fir::BinaryOp::Less => IntCC::UnsignedLessThan,
                forge_fir::BinaryOp::LessEq => IntCC::UnsignedLessThanOrEqual,
                forge_fir::BinaryOp::Greater => IntCC::UnsignedGreaterThan,
                forge_fir::BinaryOp::GreaterEq => IntCC::UnsignedGreaterThanOrEqual,
                _ => {
                    return Err(BackendError::UnsupportedInstruction {
                        kind: "non-comparison char operation",
                    })
                }
            };
            let result = instruction
                .result
                .ok_or_else(|| shape("char comparison has no result"))?;
            let value = cursor.ins().icmp(
                cc,
                scalar(scalars, *left)?,
                scalar(scalars, *right)?,
            );
            scalars.insert(result, value);
            return Ok(());
        }
        _ => {}
    }

    lower_c14_pattern_instruction(
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
