use std::collections::BTreeMap;

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
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

fn identity(owner: DefId, ty: Ty) -> FirFunction {
    let span = Span::new(0, 0);
    let (param, data) = local(0, ty.clone(), true);
    let loaded = FirValueId(0);
    FirFunction {
        owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(loaded),
                kind: FirInstructionKind::Load {
                    place: FirPlace::Local { local: param },
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty)]),
    }
}

#[test]
fn direct_call_dependency_is_scheduled_before_use_on_both_targets() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let callee_owner = DefId(80);
    let caller_owner = DefId(81);
    let produced = FirValueId(0);
    let called = FirValueId(1);

    let caller = FirFunction {
        owner: caller_owner,
        params: vec![],
        return_type: ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(called),
                    kind: FirInstructionKind::Call {
                        target: callee_owner,
                        args: vec![produced],
                        tail: false,
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(called),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(produced),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "41".into() },
                    },
                }],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(1),
                }),
            },
        ],
        value_types: BTreeMap::from([(produced, ty.clone()), (called, ty.clone())]),
    };

    let mut module = FirModule::default();
    module
        .functions
        .insert(callee_owner, identity(callee_owner, ty));
    module.functions.insert(caller_owner, caller);

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module(&module)
            .expect("call dependency scheduler");
        let clif = prepared
            .function(caller_owner)
            .expect("caller")
            .display()
            .to_string();
        assert!(clif.contains("call"), "{clif}");
        assert!(clif.contains("iconst.i64 41"), "{clif}");
    }
}

#[test]
fn same_clif_width_does_not_make_bool_a_byte_call_argument() {
    let span = Span::new(0, 0);
    let callee_owner = DefId(90);
    let caller_owner = DefId(91);
    let boolean = FirValueId(0);
    let called = FirValueId(1);

    let caller = FirFunction {
        owner: caller_owner,
        params: vec![],
        return_type: Ty::Byte,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(boolean),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Bool { value: true },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(called),
                    kind: FirInstructionKind::Call {
                        target: callee_owner,
                        args: vec![boolean],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(called),
            }),
        }],
        value_types: BTreeMap::from([(boolean, Ty::Bool), (called, Ty::Byte)]),
    };

    let mut module = FirModule::default();
    module
        .functions
        .insert(callee_owner, identity(callee_owner, Ty::Byte));
    module.functions.insert(caller_owner, caller);

    let backend = CraneliftBackend::aarch64().expect("backend");
    assert!(
        backend.prepare_module(&module).is_err(),
        "Forge semantic type mismatch must not be accepted merely because bool and byte both lower to i8"
    );
}
