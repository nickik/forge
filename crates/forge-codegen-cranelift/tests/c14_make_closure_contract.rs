use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    CaptureMode, DefId, ExprId, FirBasicBlock, FirBlockId, FirClosure, FirClosureField,
    FirFunction, FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace,
    FirTerminator, FirValueId, IntWidth, LocalId, Span, Ty, TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn closure_ty(result: Ty) -> Ty {
    Ty::Closure {
        params: Vec::new(),
        result: Box::new(result),
    }
}

fn maker(
    result: Option<Ty>,
    capture_ty: Ty,
    capture_values: bool,
    include_metadata: bool,
) -> FirFunction {
    let capture_local = FirLocalId(0);
    let capture = FirValueId(0);
    let closure_value = FirValueId(1);
    let closure_id = ExprId(7);
    let mut instructions = vec![FirInstruction {
        span: Span::new(0, 0),
        result: Some(capture),
        kind: FirInstructionKind::Load {
            place: FirPlace::Local {
                local: capture_local,
            },
        },
    }];
    instructions.push(FirInstruction {
        span: Span::new(0, 0),
        result: result.as_ref().map(|_| closure_value),
        kind: FirInstructionKind::MakeClosure {
            closure: closure_id,
            captures: capture_values.then_some(capture).into_iter().collect(),
        },
    });

    let mut closures = BTreeMap::new();
    let mut blocks = vec![FirBasicBlock {
        id: FirBlockId(0),
        closure: None,
        instructions,
        terminator: Some(FirTerminator::Return { value: None }),
    }];
    if include_metadata {
        closures.insert(
            closure_id,
            FirClosure {
                id: closure_id,
                captures: vec![FirClosureField {
                    local: LocalId(0),
                    ty: u32_ty(),
                    mode: CaptureMode::Value,
                }],
                params: Vec::new(),
                return_type: Ty::Void,
                entry: FirBlockId(1),
                function_pointer: false,
            },
        );
        blocks.push(FirBasicBlock {
            id: FirBlockId(1),
            closure: Some(closure_id),
            instructions: Vec::new(),
            terminator: Some(FirTerminator::Return { value: None }),
        });
    }

    let mut value_types = BTreeMap::from([(capture, capture_ty.clone())]);
    if let Some(result) = result {
        value_types.insert(closure_value, result);
    }
    FirFunction {
        owner: DefId(1),
        params: vec![capture_local],
        return_type: Ty::Void,
        locals: BTreeMap::from([(
            capture_local,
            FirLocal {
                id: capture_local,
                source: Some(LocalId(0)),
                ty: capture_ty,
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures,
        entry: FirBlockId(0),
        blocks,
        value_types,
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed make-closure FIR unexpectedly lowered"),
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
fn make_closure_requires_a_result() {
    assert_invalid(
        maker(None, u32_ty(), true, true),
        "make-closure has no result",
    );
}

#[test]
fn make_closure_requires_local_body_metadata() {
    assert_invalid(
        maker(Some(closure_ty(Ty::Void)), u32_ty(), true, false),
        "make-closure references missing body ExprId(7)",
    );
}

#[test]
fn make_closure_result_must_match_the_body_signature() {
    assert_invalid(
        maker(Some(closure_ty(u32_ty())), u32_ty(), true, true),
        "make-closure result type Closure { params: [], result: Int { signed: false, width: W32 } } differs from body signature Closure { params: [], result: Void }",
    );
}

#[test]
fn make_closure_capture_count_must_match_the_environment() {
    assert_invalid(
        maker(Some(closure_ty(Ty::Void)), u32_ty(), false, true),
        "make-closure has 0 capture(s), expected 1",
    );
}

#[test]
fn make_closure_capture_type_must_match_the_environment_field() {
    assert_invalid(
        maker(Some(closure_ty(Ty::Void)), Ty::Byte, true, true),
        "make-closure capture 0 has type Byte, expected Int { signed: false, width: W32 }",
    );
}

#[test]
fn make_closure_capture_mode_determines_the_environment_storage_type() {
    let mut function = maker(Some(closure_ty(Ty::Void)), u32_ty(), true, true);
    function
        .closures
        .get_mut(&ExprId(7))
        .expect("closure metadata")
        .captures[0]
        .mode = CaptureMode::MutableReference;
    assert_invalid(
        function,
        "make-closure capture 0 has type Int { signed: false, width: W32 }, expected Reference { mutable: true, inner: Int { signed: false, width: W32 } }",
    );
}
