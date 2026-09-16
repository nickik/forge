use forge_fir::{CaptureMode, ExprId};

#[derive(Clone)]
struct C14ClosureFieldLayout {
    offset: u64,
    stored_ty: Ty,
}

#[derive(Clone)]
struct C14ClosureEnvironment {
    slot: StackSlot,
    tag: u64,
    fields: Vec<C14ClosureFieldLayout>,
}

struct C14ClosureCandidate {
    closure: ExprId,
    setup: Block,
    blocks: BTreeMap<FirBlockId, Block>,
}

#[derive(Clone)]
struct C14ClosureResult {
    id: FirValueId,
    ty: Ty,
    slot: StackSlot,
}

/// C14 lowers Forge v1 captured closures as strictly function-local values.
///
/// A closure value is a pointer to a stack environment owned by the enclosing
/// native function. Calls dispatch to cloned closure FIR blocks inside that
/// same CLIF function. No closure environment is heap allocated and no
/// closure calling convention or externally visible closure symbol exists.
pub(crate) fn lower_function_c14(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if fir.closures.is_empty() {
        return lower_function_c11c(fir, all_functions, all_globals, definitions, types, isa);
    }

    if fir.closures.values().any(|closure| closure.function_pointer) {
        return Err(BackendError::UnsupportedFir {
            component: "capture-free anonymous function values before C14 function-value stage",
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
    let local_slots = c14_allocate_local_slots(fir, &mut layouts, types, &mut function)?;
    let environments =
        c14_allocate_closure_environments(fir, &mut layouts, types, &mut function)?;
    let flags = MemoryFlags {
        stack: MemFlagsData::trusted(),
        deref: MemFlagsData::new(),
    };

    // Closure blocks are deliberately not part of the enclosing function's
    // ordinary CFG. They are cloned into the CLIF CFG only at local closure
    // call sites.
    let mut blocks = BTreeMap::new();
    for fir_block in fir.blocks.iter().filter(|block| block.closure.is_none()) {
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

    for fir_block in fir.blocks.iter().filter(|block| block.closure.is_none()) {
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing CLIF block for {:?}", fir_block.id)))?;
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(clif_block);
        for instruction in &fir_block.instructions {
            lower_c14_instruction(
                fir,
                all_functions,
                all_globals,
                definitions,
                instruction,
                &local_slots,
                &environments,
                None,
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
            "CLIF verifier rejected C14 closure FIR function {:?}: {errors}",
            fir.owner
        ),
    })?;
    Ok(function)
}

fn c14_allocate_local_slots(
    fir: &FirFunction,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    function: &mut Function,
) -> Result<BTreeMap<FirLocalId, StackSlot>, BackendError> {
    let mut result = BTreeMap::new();
    for (id, local) in &fir.locals {
        let (size, align) = c14_size_align(&local.ty, layouts, types)?;
        let size = u32::try_from(size.max(1))
            .map_err(|_| shape("local layout exceeds CLIF stack-slot size"))?;
        let slot = function.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            align_shift(align)?,
        ));
        result.insert(*id, slot);
    }
    Ok(result)
}

fn c14_allocate_closure_environments(
    fir: &FirFunction,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
    function: &mut Function,
) -> Result<BTreeMap<ExprId, C14ClosureEnvironment>, BackendError> {
    let pointer_bytes = u64::from(types.target().pointer_bits / 8);
    let mut result = BTreeMap::new();

    for (tag_index, (id, closure)) in fir.closures.iter().enumerate() {
        if closure.function_pointer {
            continue;
        }

        let mut offset = pointer_bytes;
        let mut align = pointer_bytes;
        let mut fields = Vec::with_capacity(closure.captures.len());

        for capture in &closure.captures {
            let stored_ty = match capture.mode {
                CaptureMode::Value => capture.ty.clone(),
                CaptureMode::SharedReference => Ty::Reference {
                    mutable: false,
                    inner: Box::new(capture.ty.clone()),
                },
                CaptureMode::MutableReference => Ty::Reference {
                    mutable: true,
                    inner: Box::new(capture.ty.clone()),
                },
            };
            let (field_size, field_align) = c14_size_align(&stored_ty, layouts, types)?;
            offset = c14_align_up(offset, field_align)?;
            fields.push(C14ClosureFieldLayout { offset, stored_ty });
            offset = offset
                .checked_add(field_size)
                .ok_or_else(|| shape("closure environment size overflow"))?;
            align = align.max(field_align);
        }

        let size = c14_align_up(offset, align)?.max(pointer_bytes);
        let size = u32::try_from(size)
            .map_err(|_| shape("closure environment exceeds CLIF stack-slot size"))?;
        let slot = function.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            align_shift(align)?,
        ));
        result.insert(
            *id,
            C14ClosureEnvironment {
                slot,
                tag: u64::try_from(tag_index + 1).map_err(|_| shape("too many local closures"))?,
                fields,
            },
        );
    }

    Ok(result)
}

fn c14_size_align(
    ty: &Ty,
    layouts: &mut LayoutEngine<'_>,
    types: &TypeLowering<'_>,
) -> Result<(u64, u64), BackendError> {
    if matches!(ty, Ty::Closure { .. }) {
        let bytes = u64::from(types.target().pointer_bits / 8);
        return Ok((bytes, bytes));
    }
    let layout = layouts.layout_of(ty).map_err(layout_error)?;
    Ok((layout.size, layout.align))
}

fn c14_align_up(value: u64, align: u64) -> Result<u64, BackendError> {
    if align == 0 || !align.is_power_of_two() {
        return Err(shape("invalid closure environment alignment"));
    }
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or_else(|| shape("closure environment alignment overflow"))
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_instruction(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    environments: &BTreeMap<ExprId, C14ClosureEnvironment>,
    active_environment: Option<(ExprId, Value)>,
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
        FirInstructionKind::MakeClosure { closure, captures } => {
            return lower_c14_make_closure(
                fir,
                instruction,
                *closure,
                captures,
                environments,
                flags,
                scalars,
                aggregates,
                types,
                layouts,
                cursor,
            );
        }
        FirInstructionKind::CallClosure {
            closure,
            args,
            tail,
        } => {
            if *tail {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "tail closure call is outside Forge v1 C14 requirements",
                });
            }
            return lower_c14_call_closure(
                fir,
                all_functions,
                all_globals,
                definitions,
                instruction,
                *closure,
                args,
                local_slots,
                environments,
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
        FirInstructionKind::Load {
            place: FirPlace::Local { local },
        } if fir
            .locals
            .get(local)
            .is_some_and(|local| matches!(&local.ty, Ty::Closure { .. })) =>
        {
            let id = instruction
                .result
                .ok_or_else(|| shape("closure local load has no result"))?;
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing closure local stack slot {local:?}")))?;
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            let value = cursor
                .ins()
                .load(types.pointer_type()?, flags.stack, address, 0);
            scalars.insert(id, value);
            return Ok(());
        }
        FirInstructionKind::Store {
            place: FirPlace::Local { local },
            value,
        } if fir
            .locals
            .get(local)
            .is_some_and(|local| matches!(&local.ty, Ty::Closure { .. })) =>
        {
            if instruction.result.is_some() {
                return Err(shape("closure local store unexpectedly has a result"));
            }
            if !matches!(value_type(fir, *value)?, Ty::Closure { .. }) {
                return Err(shape("closure local store value is not a closure"));
            }
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing closure local stack slot {local:?}")))?;
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            cursor
                .ins()
                .store(flags.stack, scalar(scalars, *value)?, address, 0);
            return Ok(());
        }
        FirInstructionKind::Load { place }
        | FirInstructionKind::Store { place, .. }
        | FirInstructionKind::AddressOf { place, .. }
            if c14_place_uses_capture(place) =>
        {
            let (closure_id, environment) = active_environment
                .ok_or_else(|| shape("closure capture place used outside its local closure body"))?;
            return lower_c14_capture_instruction(
                fir,
                definitions,
                instruction,
                place,
                closure_id,
                environment,
                local_slots,
                environments,
                flags,
                scalars,
                aggregates,
                types,
                layouts,
                cursor,
            );
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

#[allow(clippy::too_many_arguments)]
fn lower_c14_make_closure(
    fir: &FirFunction,
    instruction: &FirInstruction,
    closure_id: ExprId,
    captures: &[FirValueId],
    environments: &BTreeMap<ExprId, C14ClosureEnvironment>,
    flags: MemoryFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result = instruction
        .result
        .ok_or_else(|| shape("make-closure has no result"))?;
    if !matches!(value_type(fir, result)?, Ty::Closure { .. }) {
        return Err(shape("make-closure result is not closure typed"));
    }
    let environment = environments
        .get(&closure_id)
        .ok_or_else(|| shape(format!("missing local closure environment {closure_id:?}")))?;
    if captures.len() != environment.fields.len() {
        return Err(shape("closure capture count differs from environment layout"));
    }

    let base = cursor
        .ins()
        .stack_addr(types.pointer_type()?, environment.slot, 0);
    let tag = cursor
        .ins()
        .iconst(types.pointer_type()?, environment.tag as i64);
    cursor.ins().store(flags.stack, tag, base, 0);

    for (capture, field) in captures.iter().zip(&environment.fields) {
        let actual = value_type(fir, *capture)?;
        if actual != &field.stored_ty {
            return Err(shape(format!(
                "closure capture type {actual:?} differs from environment field {:?}",
                field.stored_ty
            )));
        }
        let destination = add_offset(base, field.offset, cursor)?;
        store_typed_value(
            *capture,
            actual,
            destination,
            flags.stack,
            flags.stack,
            scalars,
            aggregates,
            layouts,
            cursor,
        )?;
    }

    scalars.insert(result, base);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_call_closure(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    callee: FirValueId,
    args: &[FirValueId],
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    environments: &BTreeMap<ExprId, C14ClosureEnvironment>,
    flags: MemoryFlags,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee_ty = value_type(fir, callee)?;
    let Ty::Closure {
        params,
        result: result_ty,
    } = callee_ty
    else {
        return Err(shape("call-closure callee is not closure typed"));
    };
    if args.len() != params.len() {
        return Err(shape("closure call argument count mismatch"));
    }
    for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
        let actual = value_type(fir, *arg)?;
        if actual != expected {
            return Err(shape(format!(
                "closure call argument {index} has type {actual:?}, expected {expected:?}"
            )));
        }
    }

    let candidates = fir
        .closures
        .iter()
        .filter(|(_, closure)| {
            !closure.function_pointer
                && closure.return_type == **result_ty
                && closure.params.len() == params.len()
                && closure
                    .params
                    .iter()
                    .zip(params)
                    .all(|(local, expected)| {
                        fir.locals
                            .get(local)
                            .is_some_and(|local| &local.ty == expected)
                    })
        })
        .map(|(id, _)| *id)
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(shape("closure call has no compatible local closure body"));
    }

    let closure_value = scalar(scalars, callee)?;
    let continuation = cursor.func.dfg.make_block();
    cursor.func.layout.append_block(continuation);

    let result = if **result_ty == Ty::Void {
        if instruction.result.is_some() {
            return Err(shape("void closure call unexpectedly has a result"));
        }
        None
    } else {
        let id = instruction
            .result
            .ok_or_else(|| shape("non-void closure call has no result"))?;
        if value_type(fir, id)? != result_ty.as_ref() {
            return Err(shape("closure call result type mismatch"));
        }
        let (size, align) = c14_size_align(result_ty, layouts, types)?;
        let size = u32::try_from(size.max(1))
            .map_err(|_| shape("closure result exceeds CLIF stack-slot size"))?;
        let slot = cursor.func.create_sized_stack_slot(StackSlotData::new(
            StackSlotKind::ExplicitSlot,
            size,
            align_shift(align)?,
        ));
        Some(C14ClosureResult {
            id,
            ty: result_ty.as_ref().clone(),
            slot,
        })
    };

    let mut lowered_candidates = Vec::with_capacity(candidates.len());
    for closure_id in candidates {
        let closure = fir
            .closures
            .get(&closure_id)
            .ok_or_else(|| shape(format!("missing closure {closure_id:?}")))?;
        let setup = cursor.func.dfg.make_block();
        cursor.func.layout.append_block(setup);
        let mut blocks = BTreeMap::new();
        for source in fir
            .blocks
            .iter()
            .filter(|block| block.closure == Some(closure_id))
        {
            let block = cursor.func.dfg.make_block();
            cursor.func.layout.append_block(block);
            blocks.insert(source.id, block);
        }
        if !blocks.contains_key(&closure.entry) {
            return Err(shape(format!(
                "closure {closure_id:?} is missing its entry block {:?}",
                closure.entry
            )));
        }
        lowered_candidates.push(C14ClosureCandidate {
            closure: closure_id,
            setup,
            blocks,
        });
    }

    let tag = cursor
        .ins()
        .load(types.pointer_type()?, flags.stack, closure_value, 0);
    for (index, candidate) in lowered_candidates.iter().enumerate() {
        if index + 1 == lowered_candidates.len() {
            cursor.ins().jump(candidate.setup, &[]);
        } else {
            let next = cursor.func.dfg.make_block();
            cursor.func.layout.append_block(next);
            let expected = environments
                .get(&candidate.closure)
                .ok_or_else(|| shape("missing closure environment during dispatch"))?
                .tag;
            let expected = cursor
                .ins()
                .iconst(types.pointer_type()?, expected as i64);
            let matches = cursor.ins().icmp(IntCC::Equal, tag, expected);
            cursor
                .ins()
                .brif(matches, candidate.setup, &[], next, &[]);
            cursor.goto_bottom(next);
        }
    }

    for candidate in &lowered_candidates {
        let closure = fir
            .closures
            .get(&candidate.closure)
            .ok_or_else(|| shape("closure disappeared during lowering"))?;
        cursor.goto_bottom(candidate.setup);
        for (arg, param) in args.iter().zip(&closure.params) {
            let local = fir
                .locals
                .get(param)
                .ok_or_else(|| shape(format!("missing closure parameter {param:?}")))?;
            let slot = *local_slots
                .get(param)
                .ok_or_else(|| shape(format!("missing closure parameter slot {param:?}")))?;
            let destination = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            store_typed_value(
                *arg,
                &local.ty,
                destination,
                flags.stack,
                flags.stack,
                scalars,
                aggregates,
                layouts,
                cursor,
            )?;
        }
        cursor.ins().jump(
            *candidate
                .blocks
                .get(&closure.entry)
                .ok_or_else(|| shape("missing cloned closure entry"))?,
            &[],
        );

        let mut closure_scalars = scalars.clone();
        let mut closure_aggregates = aggregates.clone();

        for source in fir
            .blocks
            .iter()
            .filter(|block| block.closure == Some(candidate.closure))
        {
            let block = *candidate
                .blocks
                .get(&source.id)
                .ok_or_else(|| shape("missing cloned closure block"))?;
            cursor.goto_bottom(block);
            for nested in &source.instructions {
                lower_c14_instruction(
                    fir,
                    all_functions,
                    all_globals,
                    definitions,
                    nested,
                    local_slots,
                    environments,
                    Some((candidate.closure, closure_value)),
                    flags,
                    call_conv,
                    direct_functions,
                    &mut closure_scalars,
                    &mut closure_aggregates,
                    types,
                    layouts,
                    cursor,
                )?;
            }
            let terminator = source
                .terminator
                .as_ref()
                .ok_or_else(|| shape(format!("closure block {:?} has no terminator", source.id)))?;
            lower_c14_closure_terminator(
                fir,
                terminator,
                closure,
                result.as_ref(),
                continuation,
                &candidate.blocks,
                flags,
                &closure_scalars,
                &closure_aggregates,
                types,
                layouts,
                cursor,
            )?;
        }
    }

    cursor.goto_bottom(continuation);
    if let Some(result) = result {
        let address = cursor
            .ins()
            .stack_addr(types.pointer_type()?, result.slot, 0);
        if c14_is_scalar_value(&result.ty) {
            let value = cursor
                .ins()
                .load(types.value_type(&result.ty)?, flags.stack, address, 0);
            scalars.insert(result.id, value);
        } else {
            aggregates.insert(
                result.id,
                AggregateValue {
                    address,
                    ty: result.ty,
                },
            );
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_closure_terminator(
    fir: &FirFunction,
    terminator: &FirTerminator,
    closure: &forge_fir::FirClosure,
    result: Option<&C14ClosureResult>,
    continuation: Block,
    blocks: &BTreeMap<FirBlockId, Block>,
    flags: MemoryFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    match terminator {
        FirTerminator::Return { value } => {
            match (value, result) {
                (None, None) if closure.return_type == Ty::Void => {}
                (Some(value), Some(result)) => {
                    if value_type(fir, *value)? != &result.ty
                        || closure.return_type != result.ty
                    {
                        return Err(shape("closure return type mismatch"));
                    }
                    let destination = cursor
                        .ins()
                        .stack_addr(types.pointer_type()?, result.slot, 0);
                    store_typed_value(
                        *value,
                        &result.ty,
                        destination,
                        flags.stack,
                        flags.stack,
                        scalars,
                        aggregates,
                        layouts,
                        cursor,
                    )?;
                }
                _ => return Err(shape("closure return value does not match closure result")),
            }
            cursor.ins().jump(continuation, &[]);
            Ok(())
        }
        FirTerminator::Select { .. } => Err(BackendError::UnsupportedInstruction {
            kind: "select terminator before C14 select/channel stage",
        }),
        _ => legacy::lower_scalar_terminator(fir, terminator, blocks, scalars, cursor),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_capture_instruction(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    place: &FirPlace,
    closure_id: ExprId,
    environment: Value,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    environments: &BTreeMap<ExprId, C14ClosureEnvironment>,
    flags: MemoryFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    match &instruction.kind {
        FirInstructionKind::Load { .. } => {
            let id = instruction
                .result
                .ok_or_else(|| shape("closure capture load has no result"))?;
            let ty = value_type(fir, id)?;
            let (address, stored_ty, src_flags) = lower_c14_capture_place_address(
                fir,
                definitions,
                place,
                closure_id,
                environment,
                local_slots,
                environments,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            if ty != &stored_ty {
                return Err(shape("closure capture load type mismatch"));
            }
            if c14_is_scalar_value(ty) {
                let value = cursor
                    .ins()
                    .load(types.value_type(ty)?, src_flags, address, 0);
                scalars.insert(id, value);
            } else {
                let value = new_aggregate(ty, layouts, types, cursor)?;
                let size = layouts.layout_of(ty).map_err(layout_error)?.size;
                copy_bytes(
                    address,
                    src_flags,
                    value.address,
                    flags.stack,
                    size,
                    cursor,
                )?;
                aggregates.insert(id, value);
            }
            Ok(())
        }
        FirInstructionKind::Store { value, .. } => {
            if instruction.result.is_some() {
                return Err(shape("closure capture store unexpectedly has a result"));
            }
            let ty = value_type(fir, *value)?;
            let (address, stored_ty, dst_flags) = lower_c14_capture_place_address(
                fir,
                definitions,
                place,
                closure_id,
                environment,
                local_slots,
                environments,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            if ty != &stored_ty {
                return Err(shape("closure capture store type mismatch"));
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
            )
        }
        FirInstructionKind::AddressOf { mutable, .. } => {
            let id = instruction
                .result
                .ok_or_else(|| shape("closure capture address-of has no result"))?;
            let result_ty = value_type(fir, id)?;
            let (address, stored_ty, _) = lower_c14_capture_place_address(
                fir,
                definitions,
                place,
                closure_id,
                environment,
                local_slots,
                environments,
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
                } if result_mutable == mutable && inner.as_ref() == &stored_ty => {}
                _ => return Err(shape("closure capture address-of type mismatch")),
            }
            scalars.insert(id, address);
            Ok(())
        }
        _ => Err(shape("capture lowering received a non-place instruction")),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_capture_place_address(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    place: &FirPlace,
    closure_id: ExprId,
    environment: Value,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    environments: &BTreeMap<ExprId, C14ClosureEnvironment>,
    flags: MemoryFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Value, Ty, MemFlags), BackendError> {
    match place {
        FirPlace::ClosureCapture { closure, index } => {
            if *closure != closure_id {
                return Err(shape("closure body refers to another closure environment"));
            }
            let source = fir
                .closures
                .get(closure)
                .ok_or_else(|| shape(format!("missing closure {closure:?}")))?;
            let field = source
                .captures
                .get(*index as usize)
                .ok_or_else(|| shape("closure capture index out of range"))?;
            let layout = environments
                .get(closure)
                .and_then(|environment| environment.fields.get(*index as usize))
                .ok_or_else(|| shape("closure capture layout is missing"))?;
            let field_address = add_offset(environment, layout.offset, cursor)?;
            match field.mode {
                CaptureMode::Value => Ok((field_address, field.ty.clone(), flags.stack)),
                CaptureMode::SharedReference | CaptureMode::MutableReference => {
                    let address = cursor
                        .ins()
                        .load(types.pointer_type()?, flags.stack, field_address, 0);
                    Ok((address, field.ty.clone(), flags.deref))
                }
            }
        }
        FirPlace::Local { local } => {
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing local stack slot {local:?}")))?;
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
            address, volatile, ..
        } => {
            if *volatile {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "volatile raw dereference",
                });
            }
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
            let (address, base_ty, mem_flags) = lower_c14_capture_place_address(
                fir,
                definitions,
                base,
                closure_id,
                environment,
                local_slots,
                environments,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            let (address, base_ty, mem_flags) = c14_autoderef_projection_base(
                address, base_ty, mem_flags, flags, types, cursor,
            )?;
            let (field_ty, offset) = field_projection(definitions, layouts, &base_ty, field)?;
            Ok((add_offset(address, offset, cursor)?, field_ty, mem_flags))
        }
        FirPlace::Index { base, index } => {
            let (address, base_ty, mem_flags) = lower_c14_capture_place_address(
                fir,
                definitions,
                base,
                closure_id,
                environment,
                local_slots,
                environments,
                flags,
                scalars,
                types,
                layouts,
                cursor,
            )?;
            let (address, base_ty, mem_flags) = c14_autoderef_projection_base(
                address, base_ty, mem_flags, flags, types, cursor,
            )?;
            let (address, element_ty) = index_address(
                fir,
                &base_ty,
                address,
                *index,
                mem_flags,
                scalars,
                layouts,
                types,
                cursor,
            )?;
            Ok((address, element_ty, mem_flags))
        }
    }
}

fn c14_place_uses_capture(place: &FirPlace) -> bool {
    match place {
        FirPlace::ClosureCapture { .. } => true,
        FirPlace::Field { base, .. } | FirPlace::Index { base, .. } => c14_place_uses_capture(base),
        FirPlace::Local { .. } | FirPlace::Deref { .. } | FirPlace::RawDeref { .. } => false,
    }
}

fn c14_is_scalar_value(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Bool
            | Ty::Byte
            | Ty::Int { .. }
            | Ty::Pointer { .. }
            | Ty::Reference { .. }
            | Ty::Function { .. }
            | Ty::Closure { .. }
    )
}
