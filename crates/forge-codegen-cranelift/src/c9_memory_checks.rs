use forge_fir::{FirFunction, FirInstructionKind, FirPlace, Ty};

use crate::BackendError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Access {
    Read,
    Write,
}

/// Preserve FIR reference mutability invariants for the C9 aggregate path.
///
/// The scalar C7 lowerer already rejects stores through shared references. C9c
/// handles aggregate stores and field/index places before delegating to that
/// lowerer, so perform the same mechanical check once at the function boundary.
pub(crate) fn validate_c9_memory_places(fir: &FirFunction) -> Result<(), BackendError> {
    for block in &fir.blocks {
        for instruction in &block.instructions {
            match &instruction.kind {
                FirInstructionKind::Store { place, value } => {
                    validate_place(fir, place, Access::Write)?;
                    if let Some(pointee) = safe_pointee_type(fir, place)? {
                        let value_ty = fir.value_types.get(value).ok_or_else(|| {
                            invalid(format!("missing type for safe store value {value:?}"))
                        })?;
                        if value_ty != pointee {
                            return Err(invalid(format!(
                                "safe FIR store value type {value_ty:?} differs from pointee type {pointee:?}"
                            )));
                        }
                    }
                    if let Some(pointee) = raw_pointee_type(fir, place)? {
                        let value_ty = fir.value_types.get(value).ok_or_else(|| {
                            invalid(format!("missing type for raw store value {value:?}"))
                        })?;
                        if value_ty != pointee {
                            return Err(invalid(format!(
                                "raw FIR store value type {value_ty:?} differs from pointee type {pointee:?}"
                            )));
                        }
                    }
                }
                FirInstructionKind::Load { place } => {
                    validate_place(fir, place, Access::Read)?;
                    if let Some(pointee) = safe_pointee_type(fir, place)? {
                        let result = instruction
                            .result
                            .ok_or_else(|| invalid("safe FIR load has no result"))?;
                        let result_ty = fir.value_types.get(&result).ok_or_else(|| {
                            invalid(format!("missing type for safe load result {result:?}"))
                        })?;
                        if result_ty != pointee {
                            return Err(invalid(format!(
                                "safe FIR load result type {result_ty:?} differs from pointee type {pointee:?}"
                            )));
                        }
                    }
                    if let Some(pointee) = raw_pointee_type(fir, place)? {
                        let result = instruction
                            .result
                            .ok_or_else(|| invalid("raw FIR load has no result"))?;
                        let result_ty = fir.value_types.get(&result).ok_or_else(|| {
                            invalid(format!("missing type for raw load result {result:?}"))
                        })?;
                        if result_ty != pointee {
                            return Err(invalid(format!(
                                "raw FIR load result type {result_ty:?} differs from pointee type {pointee:?}"
                            )));
                        }
                    }
                }
                FirInstructionKind::AddressOf { place, mutable } => {
                    validate_place(
                        fir,
                        place,
                        if *mutable {
                            Access::Write
                        } else {
                            Access::Read
                        },
                    )?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_place(fir: &FirFunction, place: &FirPlace, access: Access) -> Result<(), BackendError> {
    match place {
        FirPlace::Local { .. } => Ok(()),
        FirPlace::Field { base, .. } | FirPlace::Index { base, .. } => {
            validate_place(fir, base, access)
        }
        FirPlace::Deref { address } => {
            let ty = fir.value_types.get(address).ok_or_else(|| {
                invalid(format!(
                    "missing type for safe dereference address {address:?}"
                ))
            })?;
            match ty {
                Ty::Reference { mutable, .. } => {
                    if access == Access::Write && !*mutable {
                        return Err(invalid("write through shared FIR reference"));
                    }
                    Ok(())
                }
                _ => Err(invalid(format!(
                    "safe FIR dereference address has non-reference type {ty:?}"
                ))),
            }
        }
        FirPlace::RawDeref {
            address, volatile, ..
        } => {
            raw_deref_pointee_type(fir, *address, *volatile)?;
            Ok(())
        }
        FirPlace::ClosureCapture { .. } => Ok(()),
    }
}

fn safe_pointee_type<'a>(
    fir: &'a FirFunction,
    place: &FirPlace,
) -> Result<Option<&'a Ty>, BackendError> {
    let FirPlace::Deref { address } = place else {
        return Ok(None);
    };
    let ty = fir.value_types.get(address).ok_or_else(|| {
        invalid(format!(
            "missing type for safe dereference address {address:?}"
        ))
    })?;
    match ty {
        Ty::Reference { inner, .. } => Ok(Some(inner)),
        _ => Err(invalid(format!(
            "safe FIR dereference address has non-reference type {ty:?}"
        ))),
    }
}

fn raw_pointee_type<'a>(
    fir: &'a FirFunction,
    place: &FirPlace,
) -> Result<Option<&'a Ty>, BackendError> {
    let FirPlace::RawDeref {
        address, volatile, ..
    } = place
    else {
        return Ok(None);
    };
    raw_deref_pointee_type(fir, *address, *volatile).map(Some)
}

fn raw_deref_pointee_type(
    fir: &FirFunction,
    address: forge_fir::FirValueId,
    volatile: bool,
) -> Result<&Ty, BackendError> {
    let ty = fir.value_types.get(&address).ok_or_else(|| {
        invalid(format!(
            "missing type for raw dereference address {address:?}"
        ))
    })?;
    match ty {
        Ty::Pointer {
            volatile: pointer_volatile,
            inner,
        } => {
            if *pointer_volatile != volatile {
                return Err(invalid(format!(
                    "raw FIR dereference volatility {volatile} differs from pointer volatility {pointer_volatile}"
                )));
            }
            Ok(inner)
        }
        _ => Err(invalid(format!(
            "raw FIR dereference address has non-pointer type {ty:?}"
        ))),
    }
}

fn invalid(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use forge_fir::{
        DefId, FirBasicBlock, FirBlockId, FirInstruction, FirLocalId, FirTerminator, FirValueId,
        Span,
    };

    use super::*;

    #[test]
    fn rejects_aggregate_field_store_through_shared_reference() {
        let address = FirValueId(0);
        let value = FirValueId(1);
        let inner = Ty::Array {
            element: Box::new(Ty::Byte),
            length: Some(2),
        };
        let fir = FirFunction {
            owner: DefId(0),
            params: Vec::new(),
            return_type: Ty::Void,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 0),
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Index {
                            base: Box::new(FirPlace::Deref { address }),
                            index: FirValueId(2),
                        },
                        value,
                    },
                }],
                terminator: Some(FirTerminator::Return { value: None }),
            }],
            value_types: BTreeMap::from([
                (
                    address,
                    Ty::Reference {
                        mutable: false,
                        inner: Box::new(inner),
                    },
                ),
                (value, Ty::Byte),
                (
                    FirValueId(2),
                    Ty::Int {
                        signed: false,
                        width: forge_fir::IntWidth::Pointer,
                    },
                ),
            ]),
        };
        assert!(matches!(
            validate_c9_memory_places(&fir),
            Err(BackendError::InvalidFirShape { .. })
        ));
    }

    #[test]
    fn permits_read_through_shared_reference() {
        let address = FirValueId(0);
        let result = FirValueId(1);
        let fir = FirFunction {
            owner: DefId(0),
            params: Vec::new(),
            return_type: Ty::Byte,
            locals: BTreeMap::<FirLocalId, _>::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(result),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([
                (
                    address,
                    Ty::Reference {
                        mutable: false,
                        inner: Box::new(Ty::Byte),
                    },
                ),
                (result, Ty::Byte),
            ]),
        };
        validate_c9_memory_places(&fir).unwrap();
    }
}
