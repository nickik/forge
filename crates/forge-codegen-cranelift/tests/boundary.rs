use std::collections::BTreeMap;

use cranelift_codegen::isa::CallConv;
use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    BinaryOp, ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal,
    FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule, FirSelectCase,
    FirTerminator, FirUnaryOp, FirValueId, IntWidth, OverflowMode, RuntimeOperationId,
    Sia32PrivilegedOperation, Span, Ty,
};

#[test]
fn aarch64_backend_initializes() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Aarch64);
    assert_eq!(
        backend.target_triple().to_string(),
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn riscv64_backend_initializes() {
    let backend = CraneliftBackend::riscv64().expect("RISC-V64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Riscv64);
    assert_eq!(
        backend.target_triple().to_string(),
        "riscv64gc-unknown-linux-gnu"
    );
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn empty_module_prepares_empty_function_set() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let prepared = backend
        .prepare_module(&FirModule::default())
        .expect("empty FIR");
    assert!(prepared.functions().is_empty());
    assert!(prepared.globals().is_empty());
}

#[test]
fn scalar_global_prepares_plans_and_emits() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let mut module = FirModule::default();
    module.globals.insert(
        DefId(1),
        FirGlobal {
            owner: DefId(1),
            ty: Ty::Bool,
            mutable: false,
            constant: Some(ConstValue::Bool { value: true }),
        },
    );

    let prepared = backend
        .prepare_module(&module)
        .expect("global metadata prepares");
    assert!(prepared.global(DefId(1)).is_some());

    let plan = backend
        .plan_object_module(&prepared)
        .expect("global symbol plans");
    assert!(plan.global_symbol(DefId(1)).is_some());

    let object = backend
        .emit_object(&prepared, &plan)
        .expect("C11b emits global storage");
    assert_eq!(&object.bytes()[..4], b"\x7fELF");
}

#[test]
fn backend_error_display_is_stable() {
    assert_eq!(
        BackendError::UnsupportedInstruction { kind: "call" }.to_string(),
        "FIR instruction is not lowered to CLIF yet: call"
    );
}

#[test]
fn select_remains_an_explicit_backend_boundary_on_host_targets() {
    let owner = DefId(1);
    let duration = FirValueId(0);
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: Ty::Void,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![
                FirBasicBlock {
                    id: FirBlockId(0),
                    closure: None,
                    instructions: vec![FirInstruction {
                        span: Span::new(0, 3),
                        result: Some(duration),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Duration {
                                value: "1ms".into(),
                            },
                        },
                    }],
                    terminator: Some(FirTerminator::Select {
                        operation: RuntimeOperationId::SelectWait,
                        cases: vec![FirSelectCase::Timeout {
                            operation: RuntimeOperationId::SelectTimeout,
                            duration,
                            target: FirBlockId(1),
                        }],
                    }),
                },
                FirBasicBlock {
                    id: FirBlockId(1),
                    closure: None,
                    instructions: Vec::new(),
                    terminator: Some(FirTerminator::Return { value: None }),
                },
            ],
            value_types: BTreeMap::from([(duration, Ty::Duration)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("select/channel lowering is deferred"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::UnsupportedInstruction {
                kind: "select terminator",
            }
        );
    }
}

#[test]
fn sia32_privileged_fir_remains_an_explicit_host_target_boundary() {
    let owner = DefId(2);
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: Ty::Void,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 3),
                    result: None,
                    kind: FirInstructionKind::Sia32Privileged {
                        operation: Sia32PrivilegedOperation::Trap { imm8: 0x40 },
                        args: Vec::new(),
                    },
                }],
                terminator: Some(FirTerminator::Return { value: None }),
            }],
            value_types: BTreeMap::new(),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("SIA32 privileged FIR has no hosted-target semantics"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::UnsupportedInstruction {
                kind: "SIA32 privileged operation on non-SIA32 target",
            }
        );
    }
}

#[test]
fn non_comparison_char_fir_is_an_invalid_producer_contract() {
    let owner = DefId(3);
    let left = FirValueId(0);
    let right = FirValueId(1);
    let result = FirValueId(2);
    let span = Span::new(0, 3);
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: Ty::Char,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span,
                        result: Some(left),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Char { value: 'a' },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(right),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Char { value: 'b' },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(result),
                        kind: FirInstructionKind::Binary {
                            op: BinaryOp::Add,
                            overflow: Some(OverflowMode::Checked),
                            left,
                            right,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(left, Ty::Char), (right, Ty::Char), (result, Ty::Char)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("non-comparison char FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: "char FIR permits comparison operations only; got Add".into(),
            }
        );
    }
}

#[test]
fn invalid_subsequence_types_are_a_producer_contract_error() {
    let owner = DefId(4);
    let local = FirLocalId(0);
    let base = FirValueId(0);
    let result = FirValueId(1);
    let source_ty = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Byte),
    };
    let result_ty = Ty::Slice {
        mutable: false,
        element: Box::new(Ty::Bool),
    };
    let expected = concat!(
        "invalid subsequence from Slice { mutable: false, element: Byte } ",
        "to Slice { mutable: false, element: Bool } at start 1"
    );
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: vec![local],
            return_type: Ty::Void,
            locals: BTreeMap::from([(
                local,
                FirLocal {
                    id: local,
                    source: None,
                    ty: source_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            )]),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 3),
                        result: Some(base),
                        kind: FirInstructionKind::Load {
                            place: forge_fir::FirPlace::Local { local },
                        },
                    },
                    FirInstruction {
                        span: Span::new(4, 7),
                        result: Some(result),
                        kind: FirInstructionKind::Subsequence { base, start: 1 },
                    },
                ],
                terminator: Some(FirTerminator::Return { value: None }),
            }],
            value_types: BTreeMap::from([(base, source_ty), (result, result_ty)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("invalid subsequence FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: expected.into(),
            }
        );
    }
}

#[test]
fn floating_point_remainder_fir_is_an_invalid_producer_contract() {
    let owner = DefId(5);
    let left = FirValueId(0);
    let right = FirValueId(1);
    let result = FirValueId(2);
    let float = Ty::Float { bits: 32 };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: float.clone(),
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 3),
                        result: Some(left),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Float {
                                text: "5.5f32".into(),
                            },
                        },
                    },
                    FirInstruction {
                        span: Span::new(4, 7),
                        result: Some(right),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Float {
                                text: "2.0f32".into(),
                            },
                        },
                    },
                    FirInstruction {
                        span: Span::new(8, 9),
                        result: Some(result),
                        kind: FirInstructionKind::Binary {
                            op: BinaryOp::Rem,
                            overflow: Some(OverflowMode::Checked),
                            left,
                            right,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([
                (left, float.clone()),
                (right, float.clone()),
                (result, float),
            ]),
        },
    );

    let backend = CraneliftBackend::aarch64().expect("AArch64 backend");
    let error = match backend.prepare_module(&module) {
        Ok(_) => panic!("floating-point remainder FIR unexpectedly lowered"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::InvalidFirShape {
            message: "invalid float binary FIR operation Rem".into(),
        }
    );
}

#[test]
fn logical_not_on_non_bool_fir_is_an_invalid_producer_contract() {
    let owner = DefId(6);
    let input = FirValueId(0);
    let result = FirValueId(1);
    let integer = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: Ty::Bool,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 4),
                        result: Some(input),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Integer {
                                text: "1u32".into(),
                            },
                        },
                    },
                    FirInstruction {
                        span: Span::new(5, 6),
                        result: Some(result),
                        kind: FirInstructionKind::Unary {
                            op: FirUnaryOp::Not,
                            value: input,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(input, integer), (result, Ty::Bool)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("non-boolean logical-not FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: concat!(
                    "logical-not operand FirValueId(0) has non-bool FIR type ",
                    "Int { signed: false, width: W32 }"
                )
                .into(),
            }
        );
    }
}

#[test]
fn bitwise_not_on_non_integer_fir_is_an_invalid_producer_contract() {
    let owner = DefId(7);
    let input = FirValueId(0);
    let result = FirValueId(1);
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: Ty::Bool,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 4),
                        result: Some(input),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Bool { value: true },
                        },
                    },
                    FirInstruction {
                        span: Span::new(5, 6),
                        result: Some(result),
                        kind: FirInstructionKind::Unary {
                            op: FirUnaryOp::BitNot,
                            value: input,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(input, Ty::Bool), (result, Ty::Bool)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("non-integer bitwise-not FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: "integer unary operand FirValueId(0) has non-integer FIR type Bool".into(),
            }
        );
    }
}

#[test]
fn bitwise_not_on_float_fir_is_an_invalid_producer_contract() {
    let owner = DefId(8);
    let input = FirValueId(0);
    let result = FirValueId(1);
    let float = Ty::Float { bits: 32 };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: float.clone(),
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span: Span::new(0, 6),
                        result: Some(input),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Float {
                                text: "1.0f32".into(),
                            },
                        },
                    },
                    FirInstruction {
                        span: Span::new(7, 8),
                        result: Some(result),
                        kind: FirInstructionKind::Unary {
                            op: FirUnaryOp::BitNot,
                            value: input,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(input, float.clone()), (result, float)]),
        },
    );

    let backend = CraneliftBackend::aarch64().expect("AArch64 backend");
    let error = match backend.prepare_module(&module) {
        Ok(_) => panic!("floating-point bitwise-not FIR unexpectedly lowered"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::InvalidFirShape {
            message: "invalid float unary FIR operation BitNot".into(),
        }
    );
}

#[test]
fn float_constant_with_integer_result_type_is_an_invalid_producer_contract() {
    let owner = DefId(9);
    let result = FirValueId(0);
    let integer = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: integer.clone(),
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 6),
                    result: Some(result),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Float {
                            text: "1.0f32".into(),
                        },
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(result, integer)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("float constant with integer FIR type unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: concat!(
                    "float constant \"1.0f32\" has non-float FIR result type ",
                    "Int { signed: false, width: W32 }"
                )
                .into(),
            }
        );
    }
}

#[test]
fn boolean_constant_with_integer_result_type_is_an_invalid_producer_contract() {
    let owner = DefId(10);
    let result = FirValueId(0);
    let integer = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: integer.clone(),
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 4),
                    result: Some(result),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Bool { value: true },
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(result, integer)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("boolean constant with integer FIR type unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: concat!(
                    "boolean constant true has non-bool FIR result type ",
                    "Int { signed: false, width: W32 }"
                )
                .into(),
            }
        );
    }
}

#[test]
fn duration_constant_with_integer_result_type_is_an_invalid_producer_contract() {
    let owner = DefId(11);
    let result = FirValueId(0);
    let integer = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut module = FirModule::default();
    module.functions.insert(
        owner,
        FirFunction {
            owner,
            params: Vec::new(),
            return_type: integer.clone(),
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![FirInstruction {
                    span: Span::new(0, 3),
                    result: Some(result),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Duration {
                            value: "1ms".into(),
                        },
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(result),
                }),
            }],
            value_types: BTreeMap::from([(result, integer)]),
        },
    );

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("duration constant with integer FIR type unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: concat!(
                    "duration constant \"1ms\" has non-duration FIR result type ",
                    "Int { signed: false, width: W32 }"
                )
                .into(),
            }
        );
    }
}
