use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId,
    IntWidth, OverflowMode, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn function_ty(params: Vec<Ty>, result: Ty) -> Ty {
    Ty::Function {
        params,
        result: Box::new(result),
        named_arguments: false,
    }
}

fn local(id: u32, ty: Ty, parameter: bool) -> (FirLocalId, FirLocal) {
    let id = FirLocalId(id);
    (
        id,
        FirLocal {
            id,
            source: None,
            ty,
            mutable: false,
            parameter,
            synthetic: false,
        },
    )
}

fn targets() -> [CraneliftTarget; 2] {
    [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64]
}

fn add_one(owner: DefId) -> FirFunction {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let (param, param_local) = local(0, ty.clone(), true);
    let loaded = FirValueId(0);
    let one = FirValueId(1);
    let result = FirValueId(2);

    FirFunction {
        owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, param_local)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(one),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "1".into() },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Binary {
                        op: BinaryOp::Add,
                        overflow: Some(OverflowMode::Wrapping),
                        left: loaded,
                        right: one,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty.clone()), (one, ty.clone()), (result, ty)]),
    }
}

#[test]
fn direct_scalar_call_lowers_and_reaches_relocation_boundary_on_both_targets() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let callee_owner = DefId(10);
    let caller_owner = DefId(11);
    let callee = add_one(callee_owner);
    let (param, param_local) = local(0, ty.clone(), true);
    let loaded = FirValueId(0);
    let called = FirValueId(1);

    let caller = FirFunction {
        owner: caller_owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, param_local)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(called),
                    kind: FirInstructionKind::Call {
                        target: callee_owner,
                        args: vec![loaded],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(called),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty.clone()), (called, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_owner, callee);
    module.functions.insert(caller_owner, caller);

    for target in targets() {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module(&module)
            .expect("C8 direct call CLIF");
        let clif = prepared
            .function(caller_owner)
            .expect("caller")
            .display()
            .to_string();
        assert!(clif.contains("call"), "{clif}");
        assert!(clif.contains("u0:10"), "{clif}");

        let machine = backend
            .emit_machine_code(&prepared, caller_owner)
            .expect("direct call machine code");
        if target == CraneliftTarget::Sia32 {
            assert_eq!(machine.relocations().len(), 1);
            let relocation = &machine.relocations()[0];
            assert_eq!(relocation.target, callee_owner);
            assert_eq!(relocation.kind, cranelift_codegen::binemit::Reloc::Abs4);
        } else {
            panic!("non-SIA target unexpectedly accepted direct-call relocation");
        }
    }
}

#[test]
fn indirect_scalar_call_codegen_is_relocation_free_on_both_targets() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let fn_ty = function_ty(vec![ty.clone()], ty.clone());
    let owner = DefId(20);
    let (callee_local, callee_data) = local(0, fn_ty.clone(), true);
    let (arg_local, arg_data) = local(1, ty.clone(), true);
    let callee = FirValueId(0);
    let arg = FirValueId(1);
    let result = FirValueId(2);

    let function = FirFunction {
        owner,
        params: vec![callee_local, arg_local],
        return_type: ty.clone(),
        locals: BTreeMap::from([(callee_local, callee_data), (arg_local, arg_data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(callee),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: callee_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(arg),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: arg_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::CallIndirect {
                        callee,
                        args: vec![arg],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(callee, fn_ty), (arg, ty.clone()), (result, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);

    for target in targets() {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module(&module)
            .expect("C8 indirect call CLIF");
        let clif = prepared
            .function(owner)
            .expect("function")
            .display()
            .to_string();
        assert!(clif.contains("call_indirect"), "{clif}");
        let machine = backend
            .emit_machine_code(&prepared, owner)
            .expect("indirect call machine code");
        assert!(!machine.bytes().is_empty());
    }
}

#[test]
fn function_reference_uses_first_class_function_pointer_representation() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let fn_ty = function_ty(vec![ty.clone()], ty);
    let callee_owner = DefId(30);
    let caller_owner = DefId(31);
    let function_ref = FirValueId(0);

    let caller = FirFunction {
        owner: caller_owner,
        params: vec![],
        return_type: fn_ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(function_ref),
                kind: FirInstructionKind::FunctionRef {
                    target: callee_owner,
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(function_ref),
            }),
        }],
        value_types: BTreeMap::from([(function_ref, fn_ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_owner, add_one(callee_owner));
    module.functions.insert(caller_owner, caller);

    for target in targets() {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend.prepare_module(&module).expect("function ref CLIF");
        let clif = prepared
            .function(caller_owner)
            .expect("caller")
            .display()
            .to_string();
        assert!(clif.contains("func_addr"), "{clif}");
        assert!(clif.contains("u0:30"), "{clif}");
    }
}

#[test]
fn required_tail_call_remains_an_explicit_backend_boundary() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let callee_owner = DefId(40);
    let caller_owner = DefId(41);
    let (param, param_local) = local(0, ty.clone(), true);
    let loaded = FirValueId(0);
    let called = FirValueId(1);

    let caller = FirFunction {
        owner: caller_owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, param_local)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(called),
                    kind: FirInstructionKind::Call {
                        target: callee_owner,
                        args: vec![loaded],
                        tail: true,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(called),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty.clone()), (called, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_owner, add_one(callee_owner));
    module.functions.insert(caller_owner, caller);

    let backend = CraneliftBackend::aarch64().expect("backend");
    let error = match backend.prepare_module(&module) {
        Ok(_) => panic!("required tail calls must not silently become ordinary calls"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::UnsupportedInstruction {
            kind: "required tail call"
        }
    );
}
