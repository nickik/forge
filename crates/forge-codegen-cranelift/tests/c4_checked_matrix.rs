use std::collections::BTreeMap;

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, OverflowMode,
    Span, Ty,
};

fn int_ty(signed: bool, width: IntWidth) -> Ty {
    Ty::Int { signed, width }
}

fn backend(target: CraneliftTarget) -> CraneliftBackend {
    match target {
        CraneliftTarget::Aarch64 => CraneliftBackend::aarch64().expect("AArch64 backend"),
        CraneliftTarget::Riscv64 => CraneliftBackend::riscv64().expect("RV64 backend"),
    }
}

fn checked_binary(op: BinaryOp, ty: Ty) -> FirFunction {
    let owner = DefId(40);
    let a = FirLocalId(0);
    let b = FirLocalId(1);
    let va = FirValueId(0);
    let vb = FirValueId(1);
    let out = FirValueId(2);
    let span = Span::new(0, 0);

    FirFunction {
        owner,
        params: vec![a, b],
        return_type: ty.clone(),
        locals: BTreeMap::from([
            (
                a,
                FirLocal {
                    id: a,
                    source: None,
                    ty: ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                b,
                FirLocal {
                    id: b,
                    source: None,
                    ty: ty.clone(),
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
                    result: Some(va),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: a },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(vb),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: b },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(out),
                    kind: FirInstructionKind::Binary {
                        op,
                        overflow: Some(OverflowMode::Checked),
                        left: va,
                        right: vb,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(out) }),
        }],
        value_types: BTreeMap::from([(va, ty.clone()), (vb, ty.clone()), (out, ty)]),
    }
}

fn lower(function: FirFunction, target: CraneliftTarget) -> String {
    let owner = function.owner;
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    backend(target)
        .prepare_module(&module)
        .expect("checked C4 integer FIR should lower")
        .function(owner)
        .expect("lowered function")
        .display()
        .to_string()
}

#[test]
fn checked_add_sub_mul_cover_every_integer_width_signedness_and_target() {
    let widths = [
        IntWidth::W8,
        IntWidth::W16,
        IntWidth::W32,
        IntWidth::W64,
        IntWidth::Pointer,
    ];
    let ops = [
        (BinaryOp::Add, "add_overflow"),
        (BinaryOp::Sub, "sub_overflow"),
        (BinaryOp::Mul, "mul_overflow"),
    ];

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        for signed in [false, true] {
            for width in widths {
                for (op, stem) in ops {
                    let clif = lower(checked_binary(op, int_ty(signed, width)), target);
                    assert!(clif.contains(stem), "missing {stem} in:\n{clif}");
                    assert!(clif.contains("trapnz"), "missing overflow trap in:\n{clif}");
                }
            }
        }
    }
}

#[test]
fn checked_div_rem_cover_widths_signedness_and_targets() {
    let widths = [
        IntWidth::W8,
        IntWidth::W16,
        IntWidth::W32,
        IntWidth::W64,
        IntWidth::Pointer,
    ];

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        for signed in [false, true] {
            for width in widths {
                let ty = int_ty(signed, width);
                let div = lower(checked_binary(BinaryOp::Div, ty.clone()), target);
                let rem = lower(checked_binary(BinaryOp::Rem, ty), target);
                assert!(
                    div.contains(if signed { "sdiv" } else { "udiv" }),
                    "{div}"
                );
                assert!(
                    rem.contains(if signed { "srem" } else { "urem" }),
                    "{rem}"
                );
            }
        }
    }
}
