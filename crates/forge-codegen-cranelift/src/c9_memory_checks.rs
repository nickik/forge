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
                FirInstructionKind::Store { place, .. } => {
                    validate_place(fir, place, Access::Write)?;
                }
                FirInstructionKind::Load { place } => {
                    validate_place(fir, place, Access::Read)?;
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
        FirPlace::RawDeref { address, .. } => {
            let ty = fir.value_types.get(address).ok_or_else(|| {
                invalid(format!(
                    "missing type for raw dereference address {address:?}"
                ))
            })?;
            if matches!(ty, Ty::Pointer { .. }) {
                Ok(())
            } else {
                Err(invalid(format!(
                    "raw FIR dereference address has non-pointer type {ty:?}"
                )))
            }
        }
        FirPlace::ClosureCapture { .. } => Ok(()),
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
