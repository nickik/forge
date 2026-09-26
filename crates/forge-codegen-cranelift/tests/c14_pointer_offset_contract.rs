use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable, UnsafeProvenance,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn pointer(inner: Ty) -> Ty {
    Ty::Pointer {
        volatile: false,
        inner: Box::new(inner),
    }
}

fn function(pointer_ty: Ty, offset_ty: Ty, result_ty: Ty) -> FirFunction {
    let pointer_local = FirLocalId(0);
    let offset_local = FirLocalId(1);
    let pointer_value = FirValueId(0);
    let offset_value = FirValueId(1);
    let result = FirValueId(2);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![pointer_local, offset_local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([
            (
                pointer_local,
                FirLocal {
                    id: pointer_local,
                    source: None,
                    ty: pointer_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                offset_local,
                FirLocal {
                    id: offset_local,
                    source: None,
                    ty: offset_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
        ]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(pointer_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: pointer_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(offset_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: offset_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::PointerOffset {
                        pointer: pointer_value,
                        offset: offset_value,
                        subtract: false,
                        provenance: UnsafeProvenance { scope: span },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([
            (pointer_value, pointer_ty),
            (offset_value, offset_ty),
            (result, result_ty),
        ]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed pointer-offset FIR unexpectedly lowered"),
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
fn pointer_offset_requires_a_pointer_base() {
    assert_invalid(
        function(
            u(IntWidth::W32),
            u(IntWidth::Pointer),
            u(IntWidth::W32),
        ),
        "pointer offset base has non-pointer FIR type Int { signed: false, width: W32 }",
    );
}

#[test]
fn pointer_offset_result_must_match_its_base() {
    assert_invalid(
        function(
            pointer(u(IntWidth::W32)),
            u(IntWidth::Pointer),
            pointer(Ty::Byte),
        ),
        "pointer offset result type Pointer { volatile: false, inner: Byte } differs from base type Pointer { volatile: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn pointer_offset_requires_a_concrete_integer_offset() {
    let pointer_ty = pointer(u(IntWidth::W32));
    assert_invalid(
        function(pointer_ty.clone(), Ty::Bool, pointer_ty),
        "pointer offset has non-integer FIR type Bool",
    );
}
