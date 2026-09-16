fn normalize_integer_to_type(
    source_ty: &Ty,
    value: Value,
    target_clif: cranelift_codegen::ir::Type,
    types: &TypeLowering<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<Value, BackendError> {
    let source_clif = types.value_type(source_ty)?;
    let signed = match source_ty {
        Ty::Byte => false,
        Ty::Int { signed, .. } => *signed,
        _ => {
            return Err(BackendError::UnsupportedInstruction {
                kind: "integer normalization on non-integer FIR value",
            })
        }
    };

    Ok(match source_clif.bits().cmp(&target_clif.bits()) {
        std::cmp::Ordering::Equal => value,
        std::cmp::Ordering::Greater => cursor.ins().ireduce(target_clif, value),
        std::cmp::Ordering::Less if signed => cursor.ins().sextend(target_clif, value),
        std::cmp::Ordering::Less => cursor.ins().uextend(target_clif, value),
    })
}
