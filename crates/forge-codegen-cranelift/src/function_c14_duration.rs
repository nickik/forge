#[allow(clippy::too_many_arguments)]
fn lower_c14_duration_instruction(
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
            value: FirConst::Duration { value },
        } => {
            let result = instruction
                .result
                .ok_or_else(|| shape("duration constant has no result"))?;
            if value_type(fir, result)? != &Ty::Duration {
                return Err(shape("duration constant result is not duration typed"));
            }
            let micros = parse_duration_micros(value)?;
            scalars.insert(result, cursor.ins().iconst(clif_types::I64, micros));
            return Ok(());
        }
        FirInstructionKind::Load {
            place: FirPlace::Local { local },
        } if fir
            .locals
            .get(local)
            .is_some_and(|local| local.ty == Ty::Duration) =>
        {
            let result = instruction
                .result
                .ok_or_else(|| shape("duration local load has no result"))?;
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing duration local stack slot {local:?}")))?;
            let address = cursor.ins().stack_addr(types.pointer_type()?, slot, 0);
            scalars.insert(
                result,
                cursor.ins().load(clif_types::I64, flags.stack, address, 0),
            );
            return Ok(());
        }
        FirInstructionKind::Store {
            place: FirPlace::Local { local },
            value,
        } if fir
            .locals
            .get(local)
            .is_some_and(|local| local.ty == Ty::Duration) =>
        {
            if instruction.result.is_some() {
                return Err(shape("duration local store unexpectedly has a result"));
            }
            if value_type(fir, *value)? != &Ty::Duration {
                return Err(shape("duration local store value is not duration typed"));
            }
            let slot = *local_slots
                .get(local)
                .ok_or_else(|| shape(format!("missing duration local stack slot {local:?}")))?;
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
        } if value_type(fir, *left)? == &Ty::Duration
            && value_type(fir, *right)? == &Ty::Duration =>
        {
            let cc = match op {
                forge_fir::BinaryOp::Eq => IntCC::Equal,
                forge_fir::BinaryOp::NotEq => IntCC::NotEqual,
                forge_fir::BinaryOp::Less => IntCC::SignedLessThan,
                forge_fir::BinaryOp::LessEq => IntCC::SignedLessThanOrEqual,
                forge_fir::BinaryOp::Greater => IntCC::SignedGreaterThan,
                forge_fir::BinaryOp::GreaterEq => IntCC::SignedGreaterThanOrEqual,
                _ => {
                    return Err(BackendError::UnsupportedInstruction {
                        kind: "non-comparison duration operation",
                    })
                }
            };
            let result = instruction
                .result
                .ok_or_else(|| shape("duration comparison has no result"))?;
            scalars.insert(
                result,
                cursor.ins().icmp(
                    cc,
                    scalar(scalars, *left)?,
                    scalar(scalars, *right)?,
                ),
            );
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

fn parse_duration_micros(text: &str) -> Result<i64, BackendError> {
    let split = text
        .find(|ch: char| !ch.is_ascii_digit())
        .ok_or_else(|| shape("duration literal is missing a unit"))?;
    if split == 0 {
        return Err(shape("duration literal is missing a magnitude"));
    }
    let magnitude = text[..split]
        .parse::<u64>()
        .map_err(|_| shape("duration literal magnitude is invalid"))?;
    let multiplier = match &text[split..] {
        "us" => 1_u64,
        "ms" => 1_000,
        "s" => 1_000_000,
        "m" => 60_000_000,
        "h" => 3_600_000_000,
        _ => return Err(shape("duration literal has an unsupported unit")),
    };
    let micros = magnitude
        .checked_mul(multiplier)
        .ok_or_else(|| shape("duration literal overflows its native representation"))?;
    i64::try_from(micros)
        .map_err(|_| shape("duration literal overflows its native representation"))
}
