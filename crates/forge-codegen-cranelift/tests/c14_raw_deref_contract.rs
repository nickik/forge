use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable, UnsafeProvenance,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn pointer(inner: Ty, volatile: bool) -> Ty {
    Ty::Pointer {
        volatile,
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

fn raw_load(pointer_ty: Ty, result_ty: Ty, volatile: bool) -> FirFunction {
    let pointer_local = FirLocalId(0);
    let address = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![pointer_local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(pointer_local, local(pointer_local, pointer_ty.clone()))]),
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
                            local: pointer_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::RawDeref {
                            address,
                            volatile,
                            provenance: UnsafeProvenance { scope: span },
                        },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(address, pointer_ty), (result, result_ty)]),
    }
}

fn raw_store(pointer_ty: Ty, value_ty: Ty, volatile: bool) -> FirFunction {
    let pointer_local = FirLocalId(0);
    let value_local = FirLocalId(1);
    let address = FirValueId(0);
    let value = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![pointer_local, value_local],
        return_type: Ty::Void,
        locals: BTreeMap::from([
            (pointer_local, local(pointer_local, pointer_ty.clone())),
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
                            local: pointer_local,
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
                        place: FirPlace::RawDeref {
                            address,
                            volatile,
                            provenance: UnsafeProvenance { scope: span },
                        },
                        value,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(address, pointer_ty), (value, value_ty)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed raw-dereference FIR unexpectedly lowered"),
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
fn raw_dereference_volatility_must_match_its_pointer() {
    assert_invalid(
        raw_load(pointer(u32_ty(), false), u32_ty(), true),
        "raw FIR dereference volatility true differs from pointer volatility false",
    );
}

#[test]
fn raw_load_result_must_match_its_pointee() {
    assert_invalid(
        raw_load(pointer(u32_ty(), false), Ty::Byte, false),
        "raw FIR load result type Byte differs from pointee type Int { signed: false, width: W32 }",
    );
}

#[test]
fn raw_store_value_must_match_its_pointee() {
    assert_invalid(
        raw_store(pointer(u32_ty(), false), Ty::Byte, false),
        "raw FIR store value type Byte differs from pointee type Int { signed: false, width: W32 }",
    );
}
