#[allow(clippy::too_many_arguments)]
fn lower_c14_bitstruct_instruction(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    flags: MemoryFlags,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<bool, BackendError> {
    match &instruction.kind {
        FirInstructionKind::BitStructStorage { value, storage } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("bitstruct storage extraction has no result"))?;
            if value_type(fir, result)? != storage {
                return Err(shape("bitstruct storage result type differs from declared storage"));
            }
            let source_ty = value_type(fir, *value)?;
            let Ty::Nominal(owner) = source_ty else {
                return Err(shape("bitstruct storage extraction source is not nominal"));
            };
            let declared_storage = bitstruct_storage_type(definitions, *owner)?;
            if &declared_storage != storage {
                return Err(shape("bitstruct FIR storage differs from nominal definition"));
            }
            let source = aggregate(aggregates, *value)?;
            let raw = cursor.ins().load(
                types.value_type(storage)?,
                flags.stack,
                source.address,
                0,
            );
            scalars.insert(result, raw);
            Ok(true)
        }
        FirInstructionKind::BitStructFromStorage { value, bitstruct } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("bitstruct construction has no result"))?;
            let result_ty = value_type(fir, result)?;
            if result_ty != &Ty::Nominal(*bitstruct) {
                return Err(shape("bitstruct construction result has wrong nominal type"));
            }
            let storage = bitstruct_storage_type(definitions, *bitstruct)?;
            if value_type(fir, *value)? != &storage {
                return Err(shape("bitstruct construction source has wrong storage type"));
            }
            let aggregate_value = new_aggregate(result_ty, layouts, types, cursor)?;
            cursor.ins().store(
                flags.stack,
                scalar(scalars, *value)?,
                aggregate_value.address,
                0,
            );
            aggregates.insert(result, aggregate_value);
            Ok(true)
        }
        FirInstructionKind::BitFieldCheck { value, width } => {
            if instruction.result.is_some() {
                return Err(shape("bit-field range check unexpectedly has a result"));
            }
            let ty = value_type(fir, *value)?;
            if !matches!(ty, Ty::Byte | Ty::Int { signed: false, .. }) {
                return Err(shape("bit-field range check requires an unsigned integer"));
            }
            let bits = u32::from(types.value_type(ty)?.bits());
            if *width == 0 || *width > bits {
                return Err(shape("bit-field range check width exceeds its value type"));
            }
            if *width < bits {
                let max = (1u64 << *width) - 1;
                let out_of_range = cursor.ins().icmp_imm_u(
                    IntCC::UnsignedGreaterThan,
                    scalar(scalars, *value)?,
                    max as i64,
                );
                cursor
                    .ins()
                    .trapnz(out_of_range, TrapCode::INTEGER_OVERFLOW);
            }
            Ok(true)
        }
        // `Convert` is an explicit FIR operation. C14 therefore has concrete
        // semantics for narrowing/widening and signedness-changing integer
        // conversions instead of inheriting C4's widening-only restriction.
        FirInstructionKind::Convert { value, target }
            if matches!(value_type(fir, *value)?, Ty::Byte | Ty::Int { .. })
                && matches!(target, Ty::Byte | Ty::Int { .. }) =>
        {
            let result = instruction
                .result
                .ok_or_else(|| shape("explicit integer conversion has no result"))?;
            if value_type(fir, result)? != target {
                return Err(shape("explicit integer conversion target differs from result"));
            }
            let source_ty = value_type(fir, *value)?;
            let input = scalar(scalars, *value)?;
            let converted = normalize_integer_to_type(
                source_ty,
                input,
                types.value_type(target)?,
                types,
                cursor,
            )?;
            scalars.insert(result, converted);
            Ok(true)
        }
        // Forge source code does not permit bool/integer conversion. This FIR
        // shape is compiler-generated only when a one-bit bool bitfield is
        // packed into its unsigned storage word.
        FirInstructionKind::Convert { value, target }
            if value_type(fir, *value)? == &Ty::Bool
                && matches!(target, Ty::Byte | Ty::Int { signed: false, .. }) =>
        {
            let result = instruction
                .result
                .ok_or_else(|| shape("bit-field boolean conversion has no result"))?;
            if value_type(fir, result)? != target {
                return Err(shape("bit-field boolean conversion target differs from result"));
            }
            let input = scalar(scalars, *value)?;
            let source_ty = cursor.func.dfg.value_type(input);
            let target_ty = types.value_type(target)?;
            let converted = match source_ty.bits().cmp(&target_ty.bits()) {
                std::cmp::Ordering::Equal => input,
                std::cmp::Ordering::Greater => cursor.ins().ireduce(target_ty, input),
                std::cmp::Ordering::Less => cursor.ins().uextend(target_ty, input),
            };
            scalars.insert(result, converted);
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn bitstruct_storage_type(
    definitions: &TypeDefinitionTable,
    owner: DefId,
) -> Result<Ty, BackendError> {
    let definition = definitions
        .get(&owner)
        .ok_or_else(|| shape(format!("missing bitstruct definition {owner:?}")))?;
    match &definition.kind {
        TypeDefinitionKind::BitStruct { storage } => Ok(storage.clone()),
        _ => Err(shape(format!("nominal type {owner:?} is not a bitstruct"))),
    }
}
