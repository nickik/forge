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

    if let FirInstructionKind::PointerConvert {
        value,
        target,
        operation: _,
        provenance: _,
    } = &instruction.kind
    {
        return lower_c14_pointer_convert(
            fir,
            instruction,
            *value,
            target,
            scalars,
            types,
            cursor,
        );
    }

    if lower_c14_projection_instruction(
        fir,
        definitions,
        instruction,
        local_slots,
        flags,
        scalars,
        aggregates,
        types,
        layouts,
        cursor,
    )? {
        return Ok(());
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

fn lower_c14_pointer_convert(
    fir: &FirFunction,
    instruction: &FirInstruction,
    input: FirValueId,
    target: &Ty,
    scalars: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result = instruction
        .result
        .ok_or_else(|| shape("pointer convert has no result"))?;
    let result_ty = value_type(fir, result)?;
    if result_ty != target {
        return Err(shape("pointer convert target differs from result type"));
    }

    let source_ty = value_type(fir, input)?;
    let source = scalar(scalars, input)?;
    let source_clif = cursor.func.dfg.value_type(source);
    let target_clif = types.value_type(target)?;

    let value = match (source_ty, target) {
        (Ty::Pointer { .. }, Ty::Pointer { .. }) => {
            if source_clif != target_clif {
                return Err(shape("pointer reinterpretation changed pointer width"));
            }
            source
        }
        (Ty::Pointer { .. }, Ty::Byte | Ty::Int { .. }) => {
            match source_clif.bits().cmp(&target_clif.bits()) {
                std::cmp::Ordering::Equal => source,
                std::cmp::Ordering::Greater => cursor.ins().ireduce(target_clif, source),
                std::cmp::Ordering::Less => cursor.ins().uextend(target_clif, source),
            }
        }
        (Ty::Byte | Ty::Int { .. }, Ty::Pointer { .. }) => {
            normalize_integer_to_type(source_ty, source, target_clif, types, cursor)?
        }
        _ => {
            return Err(shape(format!(
                "invalid pointer conversion from {source_ty:?} to {target:?}"
            )))
        }
    };

    scalars.insert(result, value);
    Ok(())
}

/// C14 completes the frontend's existing auto-dereference rule for field and
/// index projections. C9c could project only a directly stored nominal/array
/// place, even though typed HIR also permits `reference.field` and
/// `reference[index]`. Lower these projections here before the legacy C9d
/// instruction path sees them.
#[allow(clippy::too_many_arguments)]
fn lower_c14_projection_instruction(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    flags: MemoryFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<bool, BackendError> {
    match &instruction.kind {
        FirInstructionKind::Load { place } if place_needs_c9(place) => {
            let id = instruction
                .result
                .ok_or_else(|| shape("projected load has no result"))?;
            let ty = value_type(fir, id)?;
            let (address, stored_ty, src_flags) = lower_c14_place_address(
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
                return Err(shape("projected load result and place types differ"));
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
            Ok(true)
        }
        FirInstructionKind::Store { place, value } if place_needs_c9(place) => {
            if instruction.result.is_some() {
                return Err(shape("projected store unexpectedly has a result"));
            }
            let ty = value_type(fir, *value)?;
            let (address, stored_ty, dst_flags) = lower_c14_place_address(
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
                return Err(shape("projected store value and place types differ"));
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
            Ok(true)
        }
        FirInstructionKind::AddressOf { place, mutable } if place_needs_c9(place) => {
            let id = instruction
                .result
                .ok_or_else(|| shape("projected address-of has no result"))?;
            let result_ty = value_type(fir, id)?;
            let (address, stored_ty, _) = lower_c14_place_address(
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
                _ => return Err(shape("projected address-of result does not match place type")),
            }
            scalars.insert(id, address);
            Ok(true)
        }
        _ => Ok(false),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_c14_place_address(
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
            let (address, base_ty, mem_flags) = lower_c14_place_address(
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
            let (address, base_ty, mem_flags) = c14_autoderef_projection_base(
                address, base_ty, mem_flags, flags, types, cursor,
            )?;
            let (field_ty, offset) = field_projection(definitions, layouts, &base_ty, field)?;
            Ok((add_offset(address, offset, cursor)?, field_ty, mem_flags))
        }
        FirPlace::Index { base, index } => {
            let (address, base_ty, mem_flags) = lower_c14_place_address(
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
        FirPlace::ClosureCapture { .. } => Err(BackendError::UnsupportedInstruction {
            kind: "closure capture place",
        }),
    }
}

fn c14_autoderef_projection_base(
    mut address: Value,
    mut ty: Ty,
    mut mem_flags: MemFlags,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Value, Ty, MemFlags), BackendError> {
    while let Ty::Reference { inner, .. } = ty {
        address = cursor
            .ins()
            .load(types.pointer_type()?, mem_flags, address, 0);
        ty = *inner;
        mem_flags = flags.deref;
    }
    Ok((address, ty, mem_flags))
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
