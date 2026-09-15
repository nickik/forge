use cranelift_codegen::ir::{ExtFuncData, ExternalName, Inst, UserExternalName};
use forge_fir::{AbiDecomposition, AbiPieceKind};

use crate::abi::{
    c9_piece_type, fir_parameter_types, lower_c9_fir_signature,
    lower_c9_function_type_signature, C9ParamPlan, C9ReturnPlan, C9SignaturePlan,
};

/// C9d replaces only the function/call ABI boundary. Aggregate memory
/// operations remain the C9c implementation above.
pub(crate) fn lower_function_c9d(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    lower_c9d_function(fir, all_functions, definitions, types, isa)
}

fn lower_c9d_function(
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
            lower_c9d_instruction(
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
        message: format!("CLIF verifier rejected C9d FIR function {:?}: {errors}", fir.owner),
    })?;
    Ok(function)
}

#[allow(clippy::too_many_arguments)]
fn initialize_c9d_parameters(
    fir: &FirFunction,
    plan: &C9SignaturePlan,
    entry_values: &[Value],
    local_slots: &BTreeMap<FirLocalId, StackSlot>,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    function: &mut Function,
) -> Result<Option<Value>, BackendError> {
    if fir.params.len() != plan.params.len() {
        return Err(shape("C9 ABI parameter plan does not match FIR parameter count"));
    }
    let entry = function
        .layout
        .blocks()
        .next()
        .ok_or_else(|| shape("C9d function has no entry block"))?;
    let mut cursor = FuncCursor::new(function);
    cursor.goto_bottom(entry);
    let mut index = 0usize;
    let hidden_return = if plan.result.is_indirect() {
        let value = *entry_values
            .get(index)
            .ok_or_else(|| shape("missing hidden aggregate return parameter"))?;
        index += 1;
        Some(value)
    } else {
        None
    };

    for ((local_id, param_plan), expected_ty) in fir
        .params
        .iter()
        .zip(&plan.params)
        .zip(fir_parameter_types(fir)?)
    {
        if param_plan.ty() != &expected_ty {
            return Err(shape("C9 ABI parameter type does not match FIR local"));
        }
        let slot = local_slots
            .get(local_id)
            .copied()
            .ok_or_else(|| shape(format!("missing parameter stack slot {local_id:?}")))?;
        let destination = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
        match param_plan {
            C9ParamPlan::Scalar { ty } => {
                let value = *entry_values
                    .get(index)
                    .ok_or_else(|| shape("missing scalar entry parameter"))?;
                index += 1;
                if cursor.func.dfg.value_type(value) != types.value_type(ty)? {
                    return Err(shape("scalar entry parameter CLIF type mismatch"));
                }
                cursor.ins().store(flags.stack, value, destination, 0);
            }
            C9ParamPlan::AggregateDirect { decomposition, .. } => {
                let count = decomposition.pieces.len();
                let pieces = entry_values
                    .get(index..index + count)
                    .ok_or_else(|| shape("missing direct aggregate entry pieces"))?;
                index += count;
                unpack_abi_pieces(
                    decomposition,
                    pieces,
                    destination,
                    flags.stack,
                    types,
                    &mut cursor,
                )?;
            }
            C9ParamPlan::AggregateIndirect { ty, .. } => {
                let source = *entry_values
                    .get(index)
                    .ok_or_else(|| shape("missing indirect aggregate entry pointer"))?;
                index += 1;
                let size = layouts.layout_of(ty).map_err(layout_error)?.size;
                copy_bytes(source, flags.deref, destination, flags.stack, size, &mut cursor)?;
            }
        }
    }
    if index != entry_values.len() {
        return Err(shape("unused CLIF entry parameters after C9 ABI unpacking"));
    }
    Ok(hidden_return)
}

#[allow(clippy::too_many_arguments)]
fn lower_c9d_instruction(
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
    match &instruction.kind {
        FirInstructionKind::Call { target, args, tail } => {
            if *tail {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "required tail call",
                });
            }
            lower_c9d_direct_call(
                fir,
                all_functions,
                definitions,
                instruction,
                *target,
                args,
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
        FirInstructionKind::CallIndirect { callee, args, tail } => {
            if *tail {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "required tail call",
                });
            }
            lower_c9d_indirect_call(
                fir,
                definitions,
                instruction,
                *callee,
                args,
                call_conv,
                scalars,
                aggregates,
                flags,
                types,
                layouts,
                cursor,
            )
        }
        FirInstructionKind::FunctionRef { target } => lower_c9d_function_ref(
            fir,
            all_functions,
            definitions,
            instruction,
            *target,
            call_conv,
            direct_functions,
            scalars,
            types,
            cursor,
        ),
        _ => lower_mixed_instruction(
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
        ),
    }
}

#[allow(clippy::too_many_arguments)]
fn lower_c9d_direct_call(
    caller: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    target: DefId,
    args: &[FirValueId],
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee = all_functions
        .get(&target)
        .ok_or_else(|| shape(format!("direct FIR call target {target:?} is not in the module")))?;
    let plan = lower_c9_fir_signature(callee, definitions, types, call_conv)?;
    let (lowered_args, indirect_result) = lower_c9d_call_arguments(
        caller,
        args,
        &plan,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )?;
    let func_ref = import_c9d_direct_function(target, &plan, direct_functions, cursor)?;
    let inst = cursor.ins().call(func_ref, &lowered_args);
    record_c9d_call_result(
        caller,
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
fn lower_c9d_indirect_call(
    caller: &FirFunction,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    callee: FirValueId,
    args: &[FirValueId],
    call_conv: CallConv,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee_ty = value_type(caller, callee)?;
    let plan = lower_c9_function_type_signature(callee_ty, definitions, types, call_conv)?;
    let (lowered_args, indirect_result) = lower_c9d_call_arguments(
        caller,
        args,
        &plan,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )?;
    let sig_ref = cursor.func.import_signature(plan.signature.clone());
    let inst = cursor
        .ins()
        .call_indirect(sig_ref, scalar(scalars, callee)?, &lowered_args);
    record_c9d_call_result(
        caller,
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
fn lower_c9d_call_arguments(
    caller: &FirFunction,
    args: &[FirValueId],
    plan: &C9SignaturePlan,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(Vec<Value>, Option<AggregateValue>), BackendError> {
    if args.len() != plan.params.len() {
        return Err(shape(format!(
            "FIR call has {} arguments but C9 ABI has {} parameters",
            args.len(),
            plan.params.len()
        )));
    }

    let indirect_result = match &plan.result {
        C9ReturnPlan::AggregateIndirect { ty, .. } => {
            Some(new_aggregate(ty, layouts, types, cursor)?)
        }
        _ => None,
    };
    let mut lowered = Vec::new();
    if let Some(result) = &indirect_result {
        lowered.push(result.address);
    }

    for (index, (arg, param)) in args.iter().zip(&plan.params).enumerate() {
        let actual_ty = value_type(caller, *arg)?;
        if actual_ty != param.ty() {
            return Err(shape(format!(
                "FIR call argument {index} has type {actual_ty:?}, expected {:?}",
                param.ty()
            )));
        }
        match param {
            C9ParamPlan::Scalar { .. } => lowered.push(scalar(scalars, *arg)?),
            C9ParamPlan::AggregateDirect { decomposition, .. } => {
                let value = aggregate(aggregates, *arg)?;
                lowered.extend(pack_abi_pieces(
                    decomposition,
                    value.address,
                    flags.stack,
                    types,
                    cursor,
                )?);
            }
            C9ParamPlan::AggregateIndirect { .. } => {
                lowered.push(aggregate(aggregates, *arg)?.address);
            }
        }
    }
    Ok((lowered, indirect_result))
}

#[allow(clippy::too_many_arguments)]
fn record_c9d_call_result(
    caller: &FirFunction,
    instruction: &FirInstruction,
    plan: &C9ReturnPlan,
    indirect_result: Option<AggregateValue>,
    inst: Inst,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    flags: MemoryFlags,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let results = cursor.func.dfg.inst_results(inst).to_vec();
    match plan {
        C9ReturnPlan::Void => {
            if !results.is_empty() {
                return Err(shape("void C9 call produced CLIF results"));
            }
            if instruction.result.is_some() {
                return Err(shape("void C9 call unexpectedly has a result value"));
            }
        }
        C9ReturnPlan::Scalar { ty } => {
            let id = call_result_id(caller, instruction, ty)?;
            let [value] = results.as_slice() else {
                return Err(shape(format!(
                    "scalar C9 call expected one CLIF result, got {}",
                    results.len()
                )));
            };
            if cursor.func.dfg.value_type(*value) != types.value_type(ty)? {
                return Err(shape("scalar C9 call result CLIF type mismatch"));
            }
            scalars.insert(id, *value);
        }
        C9ReturnPlan::AggregateDirect { ty, decomposition } => {
            let id = call_result_id(caller, instruction, ty)?;
            if results.len() != decomposition.pieces.len() {
                return Err(shape(format!(
                    "direct aggregate C9 call expected {} pieces, got {}",
                    decomposition.pieces.len(),
                    results.len()
                )));
            }
            let value = new_aggregate(ty, layouts, types, cursor)?;
            unpack_abi_pieces(
                decomposition,
                &results,
                value.address,
                flags.stack,
                types,
                cursor,
            )?;
            aggregates.insert(id, value);
        }
        C9ReturnPlan::AggregateIndirect { ty, .. } => {
            let id = call_result_id(caller, instruction, ty)?;
            if !results.is_empty() {
                return Err(shape("indirect aggregate C9 call produced CLIF results"));
            }
            let value = indirect_result
                .ok_or_else(|| shape("missing hidden result storage for aggregate call"))?;
            aggregates.insert(id, value);
        }
    }
    Ok(())
}

fn call_result_id(
    caller: &FirFunction,
    instruction: &FirInstruction,
    expected: &Ty,
) -> Result<FirValueId, BackendError> {
    let id = instruction
        .result
        .ok_or_else(|| shape("non-void C9 call has no FIR result"))?;
    let actual = value_type(caller, id)?;
    if actual != expected {
        return Err(shape(format!(
            "C9 call result type {actual:?} differs from callee result {expected:?}"
        )));
    }
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
fn lower_c9d_function_ref(
    caller: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    target: DefId,
    call_conv: CallConv,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    scalars: &mut BTreeMap<FirValueId, Value>,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let result_id = instruction
        .result
        .ok_or_else(|| shape("function-ref has no result"))?;
    let result_ty = value_type(caller, result_id)?;
    let Ty::Function {
        params,
        result,
        named_arguments: _,
    } = result_ty
    else {
        return Err(shape("function-ref result is not function typed"));
    };
    let callee = all_functions
        .get(&target)
        .ok_or_else(|| shape(format!("function-ref target {target:?} is not in module")))?;
    if params != &fir_parameter_types(callee)? || result.as_ref() != &callee.return_type {
        return Err(shape("function-ref type does not match target signature"));
    }
    let plan = lower_c9_fir_signature(callee, definitions, types, call_conv)?;
    let func_ref = import_c9d_direct_function(target, &plan, direct_functions, cursor)?;
    let address = cursor.ins().func_addr(types.pointer_type()?, func_ref);
    scalars.insert(result_id, address);
    Ok(())
}

fn import_c9d_direct_function(
    target: DefId,
    plan: &C9SignaturePlan,
    direct_functions: &mut BTreeMap<DefId, FuncRef>,
    cursor: &mut FuncCursor<'_>,
) -> Result<FuncRef, BackendError> {
    if let Some(existing) = direct_functions.get(&target) {
        return Ok(*existing);
    }
    let sig_ref = cursor.func.import_signature(plan.signature.clone());
    let user_name = cursor
        .func
        .declare_imported_user_function(UserExternalName::new(0, target.0));
    let func_ref = cursor.func.import_function(ExtFuncData {
        name: ExternalName::user(user_name),
        signature: sig_ref,
        colocated: true,
        patchable: false,
    });
    direct_functions.insert(target, func_ref);
    Ok(func_ref)
}

#[allow(clippy::too_many_arguments)]
fn lower_c9d_terminator(
    fir: &FirFunction,
    terminator: &FirTerminator,
    plan: &C9ReturnPlan,
    hidden_return: Option<Value>,
    blocks: &BTreeMap<FirBlockId, Block>,
    flags: MemoryFlags,
    scalars: &BTreeMap<FirValueId, Value>,
    aggregates: &BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    match (terminator, plan) {
        (
            FirTerminator::Return { value: Some(id) },
            C9ReturnPlan::AggregateDirect { ty, decomposition },
        ) => {
            if value_type(fir, *id)? != ty {
                return Err(shape("direct aggregate return type mismatch"));
            }
            let value = aggregate(aggregates, *id)?;
            let pieces = pack_abi_pieces(
                decomposition,
                value.address,
                flags.stack,
                types,
                cursor,
            )?;
            cursor.ins().return_(&pieces);
            Ok(())
        }
        (
            FirTerminator::Return { value: Some(id) },
            C9ReturnPlan::AggregateIndirect { ty, .. },
        ) => {
            if value_type(fir, *id)? != ty {
                return Err(shape("indirect aggregate return type mismatch"));
            }
            let destination = hidden_return
                .ok_or_else(|| shape("missing hidden aggregate return pointer"))?;
            let source = aggregate(aggregates, *id)?;
            let size = layouts.layout_of(ty).map_err(layout_error)?.size;
            copy_bytes(
                source.address,
                flags.stack,
                destination,
                flags.deref,
                size,
                cursor,
            )?;
            cursor.ins().return_(&[]);
            Ok(())
        }
        (FirTerminator::Return { value: Some(_) }, C9ReturnPlan::Void) => {
            Err(shape("void C9 function returns a value"))
        }
        (
            FirTerminator::Return { value: None },
            C9ReturnPlan::AggregateDirect { .. }
            | C9ReturnPlan::AggregateIndirect { .. }
            | C9ReturnPlan::Scalar { .. },
        ) => Err(shape("non-void C9 function returns no value")),
        _ => legacy::lower_scalar_terminator(fir, terminator, blocks, scalars, cursor),
    }
}

fn pack_abi_pieces(
    decomposition: &AbiDecomposition,
    address: Value,
    flags: MemFlags,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Vec<Value>, BackendError> {
    let mut result = Vec::with_capacity(decomposition.pieces.len());
    for piece in &decomposition.pieces {
        let piece_ty = c9_piece_type(piece, types)?;
        match piece.kind {
            AbiPieceKind::Pointer => {
                let [fragment] = piece.fragments.as_slice() else {
                    return Err(shape("pointer ABI piece must contain exactly one fragment"));
                };
                if fragment.bits != piece.bits || fragment.piece_bit_offset != 0 {
                    return Err(shape("pointer ABI fragment does not cover its piece"));
                }
                result.push(cursor.ins().load(
                    piece_ty,
                    flags,
                    address,
                    i32_offset(fragment.source_offset)?,
                ));
            }
            AbiPieceKind::Integer => {
                let mut packed = cursor.ins().iconst(piece_ty, 0);
                for fragment in &piece.fragments {
                    let fragment_ty = clif_integer_type(fragment.bits)?;
                    let raw = cursor.ins().load(
                        fragment_ty,
                        flags,
                        address,
                        i32_offset(fragment.source_offset)?,
                    );
                    let mut widened = if fragment.bits == piece.bits {
                        raw
                    } else if fragment.bits < piece.bits {
                        cursor.ins().uextend(piece_ty, raw)
                    } else {
                        return Err(shape("ABI fragment wider than containing piece"));
                    };
                    if fragment.piece_bit_offset != 0 {
                        let shift = cursor
                            .ins()
                            .iconst(piece_ty, i64::from(fragment.piece_bit_offset));
                        widened = cursor.ins().ishl(widened, shift);
                    }
                    packed = cursor.ins().bor(packed, widened);
                }
                result.push(packed);
            }
        }
    }
    Ok(result)
}

fn unpack_abi_pieces(
    decomposition: &AbiDecomposition,
    values: &[Value],
    address: Value,
    flags: MemFlags,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    if values.len() != decomposition.pieces.len() {
        return Err(shape("ABI piece count mismatch during aggregate unpack"));
    }
    for (piece, value) in decomposition.pieces.iter().zip(values) {
        let piece_ty = c9_piece_type(piece, types)?;
        if cursor.func.dfg.value_type(*value) != piece_ty {
            return Err(shape("ABI piece CLIF type mismatch during aggregate unpack"));
        }
        match piece.kind {
            AbiPieceKind::Pointer => {
                let [fragment] = piece.fragments.as_slice() else {
                    return Err(shape("pointer ABI piece must contain exactly one fragment"));
                };
                if fragment.bits != piece.bits || fragment.piece_bit_offset != 0 {
                    return Err(shape("pointer ABI fragment does not cover its piece"));
                }
                cursor.ins().store(
                    flags,
                    *value,
                    address,
                    i32_offset(fragment.source_offset)?,
                );
            }
            AbiPieceKind::Integer => {
                for fragment in &piece.fragments {
                    let mut extracted = *value;
                    if fragment.piece_bit_offset != 0 {
                        let shift = cursor
                            .ins()
                            .iconst(piece_ty, i64::from(fragment.piece_bit_offset));
                        extracted = cursor.ins().ushr(extracted, shift);
                    }
                    let fragment_ty = clif_integer_type(fragment.bits)?;
                    if fragment.bits < piece.bits {
                        extracted = cursor.ins().ireduce(fragment_ty, extracted);
                    } else if fragment.bits > piece.bits {
                        return Err(shape("ABI fragment wider than containing piece"));
                    }
                    cursor.ins().store(
                        flags,
                        extracted,
                        address,
                        i32_offset(fragment.source_offset)?,
                    );
                }
            }
        }
    }
    Ok(())
}
