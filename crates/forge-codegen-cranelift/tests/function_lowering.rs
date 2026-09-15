use std::collections::BTreeMap;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId,
    IntWidth, Span, Ty,
};

fn int_ty(signed: bool) -> Ty {
    Ty::Int {
        signed,
        width: IntWidth::W64,
    }
}

fn choose_module(signed: bool) -> (FirModule, DefId) {
    let owner = DefId(0);
    let param = FirLocalId(0);
    let integer_ty = int_ty(signed);
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let v2 = FirValueId(2);
    let v3 = FirValueId(3);
    let v4 = FirValueId(4);
    let span = Span::new(0, 0);

    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: integer_ty.clone(),
        locals: BTreeMap::from([(
            param,
            FirLocal {
                id: param,
                source: None,
                ty: integer_ty.clone(),
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span,
                        result: Some(v0),
                        kind: FirInstructionKind::Load {
                            place: FirPlace::Local { local: param },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(v1),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Integer { text: "10".into() },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(v2),
                        kind: FirInstructionKind::Binary {
                            op: BinaryOp::Greater,
                            overflow: None,
                            left: v0,
                            right: v1,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Branch {
                    condition: v2,
                    then_block: FirBlockId(1),
                    else_block: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(v3),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "1".into() },
                    },
                }],
                terminator: Some(FirTerminator::Return { value: Some(v3) }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(v4),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "2".into() },
                    },
                }],
                terminator: Some(FirTerminator::Return { value: Some(v4) }),
            },
        ],
        value_types: BTreeMap::from([
            (v0, integer_ty.clone()),
            (v1, integer_ty.clone()),
            (v2, Ty::Bool),
            (v3, integer_ty.clone()),
            (v4, integer_ty),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn cross_block_value_module() -> (FirModule, DefId) {
    let owner = DefId(1);
    let ty = int_ty(false);
    let value = FirValueId(10);
    let span = Span::new(0, 0);
    let function = FirFunction {
        owner,
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
                    target: FirBlockId(1),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(value),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "42".into() },
                    },
                }],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![],
                terminator: Some(FirTerminator::Return { value: Some(value) }),
            },
        ],
        value_types: BTreeMap::from([(value, ty)]),
    };
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn assert_choose_lowers(backend: CraneliftBackend, signed: bool, condition: &str) {
    let (module, owner) = choose_module(signed);
    let prepared = backend
        .prepare_module(&module)
        .expect("minimal choose FIR should lower and pass the CLIF verifier");
    let function = prepared.function(owner).expect("lowered choose function");
    let clif = function.display().to_string();

    assert!(clif.contains("(i64) -> i64"), "{clif}");
    assert!(clif.contains(condition), "{clif}");
    assert!(clif.contains("brif"), "{clif}");
    assert_eq!(clif.matches("return").count(), 2, "{clif}");
}

#[test]
fn unsigned_choose_lowers_to_verified_aarch64_clif() {
    assert_choose_lowers(
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        false,
        "icmp ugt",
    );
}

#[test]
fn unsigned_choose_lowers_to_verified_riscv64_clif() {
    assert_choose_lowers(
        CraneliftBackend::riscv64().expect("RISC-V64 backend"),
        false,
        "icmp ugt",
    );
}

#[test]
fn signed_comparison_keeps_fir_signedness() {
    assert_choose_lowers(
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        true,
        "icmp sgt",
    );
}

#[test]
fn dominating_scalar_value_crosses_basic_blocks() {
    let (module, owner) = cross_block_value_module();
    let backend = CraneliftBackend::aarch64().expect("AArch64 backend");
    let prepared = backend
        .prepare_module(&module)
        .expect("dominating scalar FIR value should cross blocks");
    let clif = prepared
        .function(owner)
        .expect("lowered linear function")
        .display()
        .to_string();
    assert!(clif.contains("iconst.i64 42"), "{clif}");
    assert_eq!(clif.matches("jump").count(), 2, "{clif}");
    assert!(clif.contains("return"), "{clif}");
}
