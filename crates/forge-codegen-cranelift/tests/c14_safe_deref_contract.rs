use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn reference(inner: Ty, mutable: bool) -> Ty {
    Ty::Reference {
        mutable,
        inner: Box::new(inner),
    }
}

fn local(id: FirLocalId, ty: Ty) -> FirLocal {
    FirLocal {
        id,
        source: None,
        ty,
        mutable: false,
        parameter: true,
        synthetic: false,
    }
}

fn safe_load(reference_ty: Ty, result_ty: Ty) -> FirFunction {
    let reference_local = FirLocalId(0);
    let address = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![reference_local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            reference_local,
            local(reference_local, reference_ty.clone()),
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: reference_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(address, reference_ty), (result, result_ty)]),
    }
}

fn safe_store(reference_ty: Ty, value_ty: Ty) -> FirFunction {
    let reference_local = FirLocalId(0);
    let value_local = FirLocalId(1);
    let address = FirValueId(0);
    let value = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![reference_local, value_local],
        return_type: Ty::Void,
        locals: BTreeMap::from([
            (
                reference_local,
                local(reference_local, reference_ty.clone()),
            ),
            (value_local, local(value_local, value_ty.clone())),
        ]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: reference_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: value_local },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Deref { address },
                        value,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(address, reference_ty), (value, value_ty)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed safe-dereference FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: message.into(),
            }
        );
    }
}

#[test]
fn safe_load_result_must_match_its_pointee() {
    assert_invalid(
        safe_load(reference(u32_ty(), false), Ty::Byte),
        "safe FIR load result type Byte differs from pointee type Int { signed: false, width: W32 }",
    );
}

#[test]
fn safe_store_value_must_match_its_pointee() {
    assert_invalid(
        safe_store(reference(u32_ty(), true), Ty::Byte),
        "safe FIR store value type Byte differs from pointee type Int { signed: false, width: W32 }",
    );
}
