use forge_fir::{CaptureMode, ExprId};

#[derive(Clone)]
struct C14ClosureFieldLayout {
    offset: u64,
    stored_ty: Ty,
}

#[derive(Clone)]
struct C14ClosureEnvironment {
    slot: StackSlot,
    fields: Vec<C14ClosureFieldLayout>,
}

/// C14 lowers Forge v1 captured closures as non-escaping environment pointers.
///
/// A closure value points at lexical stack storage whose first word is the
/// lifted closure code pointer and whose remaining fields are captures. A call
/// loads that code pointer and performs an indirect native call with the
/// environment pointer as the hidden first argument. No environment is heap
/// allocated, so the value may cross synchronous call boundaries but may not
/// outlive the creating activation.
pub(crate) fn lower_function_c14(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    if let Some(closure_id) = fir
        .blocks
        .iter()
        .find(|block| block.id == fir.entry)
        .and_then(|block| block.closure)
    {
        return lower_c14_lifted_closure(
            fir,
            closure_id,
            all_functions,
            all_globals,
            definitions,
            types,
            isa,
        );
    }

    let has_closure_value = fir
        .locals
        .values()
        .any(|local| matches!(&local.ty, Ty::Closure { .. }));
    let has_closure_call = fir
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .any(|instruction| matches!(&instruction.kind, FirInstructionKind::CallClosure { .. }));
    if fir.closures.is_empty() && !has_closure_value && !has_closure_call {
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

#[allow(clippy::too_many_arguments)]
fn lower_c14_lifted_closure(
    fir: &FirFunction,
    closure_id: ExprId,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    let closure = fir
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("lifted closure metadata is missing"))?;
    if fir.params.len() != closure.params.len() + 1 {
        return Err(shape("lifted closure hidden environment parameter is missing"));
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
    let environments = c14_allocate_closure_environments(fir, &mut layouts, types, &mut function)?;
    let flags = MemoryFlags {
        stack: MemFlagsData::trusted(),
        deref: MemFlagsData::new(),
    };

    let mut blocks = BTreeMap::new();
    for fir_block in fir.blocks.iter().filter(|block| block.closure == Some(closure_id)) {
        let block = function.dfg.make_block();
        function.layout.append_block(block);
        if blocks.insert(fir_block.id, block).is_some() {
            return Err(shape(format!("duplicate lifted closure FIR block {:?}", fir_block.id)));
        }
    }
    let entry = *blocks
        .get(&fir.entry)
        .ok_or_else(|| shape(format!("missing lifted closure entry block {:?}", fir.entry)))?;
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

    let env_local = *fir
        .params
        .first()
        .ok_or_else(|| shape("lifted closure has no hidden environment local"))?;
    let env_slot = *local_slots
        .get(&env_local)
        .ok_or_else(|| shape("lifted closure environment local has no stack slot"))?;
    let environment = {
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(entry);
        let address = cursor.ins().stack_addr(types.pointer_type()?, env_slot, 0);
        cursor
            .ins()
            .load(types.pointer_type()?, flags.stack, address, 0)
    };

    for fir_block in fir.blocks.iter().filter(|block| block.closure == Some(closure_id)) {
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing lifted closure CLIF block for {:?}", fir_block.id)))?;
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
                Some((closure_id, environment)),
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
            .ok_or_else(|| shape(format!("lifted closure block {:?} has no terminator", fir_block.id)))?;
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
        message: format!("CLIF verifier rejected lifted C14 closure {:?}: {errors}", fir.owner),
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

    for (id, closure) in &fir.closures {
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
            C14ClosureEnvironment { slot, fields },
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
                definitions,
                instruction,
                *closure,
                args,
                flags,
                call_conv,
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
    if captures.len() != environment.fields.len() + 1 {
        return Err(shape("closure code/capture count differs from environment layout"));
    }

    let base = cursor
        .ins()
        .stack_addr(types.pointer_type()?, environment.slot, 0);
    let code = captures[0];
    if !matches!(value_type(fir, code)?, Ty::Function { .. }) {
        return Err(shape("captured closure code value is not function typed"));
    }
    cursor
        .ins()
        .store(flags.stack, scalar(scalars, code)?, base, 0);

    for (capture, field) in captures[1..].iter().zip(&environment.fields) {
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
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    callee: FirValueId,
    args: &[FirValueId],
    flags: MemoryFlags,
    call_conv: CallConv,
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

    let mut code_params = Vec::with_capacity(params.len() + 1);
    code_params.push(callee_ty.clone());
    code_params.extend(params.iter().cloned());
    let code_ty = Ty::Function {
        params: code_params,
        result: result_ty.clone(),
        named_arguments: false,
    };
    let plan = lower_c9_function_type_signature(&code_ty, definitions, types, call_conv)?;
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(callee);
    call_args.extend(args.iter().copied());
    let (lowered_args, indirect_result) = lower_c9d_call_arguments(
        fir,
        &call_args,
        &plan,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )?;

    let environment = scalar(scalars, callee)?;
    let code = cursor
        .ins()
        .load(types.pointer_type()?, flags.deref, environment, 0);
    let sig_ref = cursor.func.import_signature(plan.signature.clone());
    let inst = cursor.ins().call_indirect(sig_ref, code, &lowered_args);
    record_c9d_call_result(
        fir,
        instruction,
        &plan.result,
        indirect_result,
        inst,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )
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
                CaptureMode::Value => Ok((field_address, field.ty.clone(), flags.deref)),
                CaptureMode::SharedReference | CaptureMode::MutableReference => {
                    let address = cursor
                        .ins()
                        .load(types.pointer_type()?, flags.deref, field_address, 0);
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
