use std::collections::BTreeMap;

use cranelift_codegen::isa::CallConv;
use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal, FirInstruction,
    FirInstructionKind, FirModule, FirSelectCase, FirTerminator, FirValueId, RuntimeOperationId,
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
