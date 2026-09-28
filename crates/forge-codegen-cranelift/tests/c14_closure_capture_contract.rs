use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    CaptureMode, DefId, ExprId, FirBasicBlock, FirBlockId, FirClosure, FirClosureField,
    FirFunction, FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace,
    FirTerminator, FirValueId, IntWidth, LocalId, Span, Ty, TypeDefinitionTable,
};

#[derive(Clone)]
enum CaptureOperation {
    Load { result_ty: Ty },
    Store { value_ty: Ty },
    AddressOf { mutable: bool, result_ty: Ty },
}

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn capture_function(
    owner: ExprId,
    index: u32,
    mode: CaptureMode,
    operation: CaptureOperation,
) -> FirFunction {
    let closure_id = ExprId(7);
    let value_local = FirLocalId(0);
    let value = FirValueId(0);
    let result = FirValueId(1);
    let value_ty = match &operation {
        CaptureOperation::Store { value_ty } => value_ty.clone(),
        CaptureOperation::Load { .. } | CaptureOperation::AddressOf { .. } => u32_ty(),
    };
    let mut value_types = BTreeMap::from([(value, value_ty.clone())]);
    let access = match operation {
        CaptureOperation::Load { result_ty } => {
            value_types.insert(result, result_ty);
            FirInstruction {
                span: Span::new(0, 0),
                result: Some(result),
                kind: FirInstructionKind::Load {
                    place: FirPlace::ClosureCapture {
                        closure: owner,
                        index,
                    },
                },
            }
        }
        CaptureOperation::Store { .. } => FirInstruction {
            span: Span::new(0, 0),
            result: None,
            kind: FirInstructionKind::Store {
                place: FirPlace::ClosureCapture {
                    closure: owner,
                    index,
                },
                value,
            },
        },
        CaptureOperation::AddressOf { mutable, result_ty } => {
            value_types.insert(result, result_ty);
            FirInstruction {
                span: Span::new(0, 0),
                result: Some(result),
                kind: FirInstructionKind::AddressOf {
                    place: FirPlace::ClosureCapture {
                        closure: owner,
                        index,
                    },
                    mutable,
                },
            }
        }
    };

    FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type: Ty::Void,
        locals: BTreeMap::from([(
            value_local,
            FirLocal {
                id: value_local,
                source: Some(LocalId(1)),
                ty: value_ty,
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::from([(
            closure_id,
            FirClosure {
                id: closure_id,
                captures: vec![FirClosureField {
                    local: LocalId(0),
                    ty: u32_ty(),
                    mode,
                }],
                params: vec![value_local],
                return_type: Ty::Void,
                entry: FirBlockId(1),
                function_pointer: false,
            },
        )]),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: Vec::new(),
                terminator: Some(FirTerminator::Return { value: None }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: Some(closure_id),
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 0),
                        result: Some(value),
                        kind: FirInstructionKind::Load {
                            place: FirPlace::Local { local: value_local },
                        },
                    },
                    access,
                ],
                terminator: Some(FirTerminator::Return { value: None }),
            },
        ],
        value_types,
    }
}

fn assert_invalid(function: FirFunction) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed closure-capture FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFir {
                diagnostic_count: 1,
            }
        );
    }
}

#[test]
fn closure_capture_must_be_used_inside_a_closure_body() {
    let mut function = capture_function(
        ExprId(7),
        0,
        CaptureMode::Value,
        CaptureOperation::Load {
            result_ty: u32_ty(),
        },
    );
    let access = function.blocks[1].instructions.pop().expect("capture load");
    function.blocks[0].instructions.push(access);
    assert_invalid(function);
}

#[test]
fn closure_capture_must_belong_to_the_active_body() {
    assert_invalid(capture_function(
        ExprId(8),
        0,
        CaptureMode::Value,
        CaptureOperation::Load {
            result_ty: u32_ty(),
        },
    ));
}

#[test]
fn closure_capture_index_must_exist() {
    assert_invalid(capture_function(
        ExprId(7),
        1,
        CaptureMode::Value,
        CaptureOperation::Load {
            result_ty: u32_ty(),
        },
    ));
}

#[test]
fn closure_capture_load_must_preserve_the_field_type() {
    assert_invalid(capture_function(
        ExprId(7),
        0,
        CaptureMode::Value,
        CaptureOperation::Load {
            result_ty: Ty::Byte,
        },
    ));
}

#[test]
fn closure_capture_store_must_preserve_the_field_type() {
    assert_invalid(capture_function(
        ExprId(7),
        0,
        CaptureMode::Value,
        CaptureOperation::Store { value_ty: Ty::Byte },
    ));
}

#[test]
fn shared_closure_capture_rejects_writes() {
    assert_invalid(capture_function(
        ExprId(7),
        0,
        CaptureMode::SharedReference,
        CaptureOperation::Store { value_ty: u32_ty() },
    ));
}

#[test]
fn closure_capture_address_must_preserve_mutability_and_field_type() {
    assert_invalid(capture_function(
        ExprId(7),
        0,
        CaptureMode::MutableReference,
        CaptureOperation::AddressOf {
            mutable: true,
            result_ty: Ty::Reference {
                mutable: false,
                inner: Box::new(u32_ty()),
            },
        },
    ));
}
