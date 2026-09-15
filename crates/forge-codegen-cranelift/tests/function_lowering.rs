use std::collections::BTreeMap;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId,
    IntWidth, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn choose_module() -> (FirModule, DefId) {
    let owner = DefId(0);
    let param = FirLocalId(0);
    let u64_ty = u64_ty();

    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let v2 = FirValueId(2);
    let v3 = FirValueId(3);
    let v4 = FirValueId(4);

    let span = Span::new(0, 0);
    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: u64_ty.clone(),
        locals: BTreeMap::from([(
            param,
            FirLocal {
                id: param,
                source: None,
                ty: u64_ty.clone(),
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
            (v0, u64_ty.clone()),
            (v1, u64_ty.clone()),
            (v2, Ty::Bool),
            (v3, u64_ty.clone()),
            (v4, u64_ty),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn assert_choose_lowers(backend: CraneliftBackend) {
    let (module, owner) = choose_module();
    let prepared = backend
        .prepare_module(&module)
        .expect("minimal choose FIR should lower and pass the CLIF verifier");
    let function = prepared.function(owner).expect("lowered choose function");
    let clif = function.display().to_string();

    assert!(clif.contains("(i64) -> i64"), "{clif}");
    assert!(clif.contains("icmp ugt"), "{clif}");
    assert!(clif.contains("brif"), "{clif}");
    assert_eq!(clif.matches("return").count(), 2, "{clif}");
}

#[test]
fn choose_lowers_to_verified_aarch64_clif() {
    assert_choose_lowers(CraneliftBackend::aarch64().expect("AArch64 backend"));
}

#[test]
fn choose_lowers_to_verified_riscv64_clif() {
    assert_choose_lowers(CraneliftBackend::riscv64().expect("RISC-V64 backend"));
}
