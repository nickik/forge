use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, OverflowMode,
    Span, Ty,
};

fn int_ty(signed: bool, width: IntWidth) -> Ty {
    Ty::Int { signed, width }
}

fn binary_function(
    owner: DefId,
    ty: Ty,
    op: BinaryOp,
    overflow: Option<OverflowMode>,
    result_ty: Ty,
) -> FirFunction {
    let left_local = FirLocalId(0);
    let right_local = FirLocalId(1);
    let left = FirValueId(0);
    let right = FirValueId(1);
    let result = FirValueId(2);
    let span = Span::new(0, 0);

    FirFunction {
        owner,
        params: vec![left_local, right_local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([
            (
                left_local,
                FirLocal {
                    id: left_local,
                    source: None,
                    ty: ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                right_local,
                FirLocal {
                    id: right_local,
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
                    result: Some(left),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: left_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(right),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: right_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Binary {
                        op,
                        overflow,
                        left,
                        right,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(left, ty.clone()), (right, ty), (result, result_ty)]),
    }
}

fn lower_single(function: FirFunction, backend: CraneliftBackend) -> Result<String, BackendError> {
    let owner = function.owner;
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    let prepared = backend.prepare_module(&module)?;
    Ok(prepared
        .function(owner)
        .expect("lowered function")
        .display()
        .to_string())
}

#[test]
fn wrapping_add_sub_mul_lower_for_all_integer_widths_on_both_targets() {
    let widths = [
        IntWidth::W8,
        IntWidth::W16,
        IntWidth::W32,
        IntWidth::W64,
        IntWidth::Pointer,
    ];
    let ops = [
        (BinaryOp::Add, "iadd"),
        (BinaryOp::Sub, "isub"),
        (BinaryOp::Mul, "imul"),
    ];

    for backend in [
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        CraneliftBackend::riscv64().expect("RISC-V64 backend"),
    ] {
        for signed in [false, true] {
            for width in widths {
                let ty = int_ty(signed, width);
                for (index, (op, clif_op)) in ops.iter().copied().enumerate() {
                    let function = binary_function(
                        DefId(index as u32),
                        ty.clone(),
                        op,
                        Some(OverflowMode::Wrapping),
                        ty.clone(),
                    );
                    let clif = lower_single(function, backend.clone_for_test())
                        .expect("wrapping C4a arithmetic should lower");
                    assert!(clif.contains(clif_op), "missing {clif_op} in:\n{clif}");
                }
            }
        }
    }
}

#[test]
fn signed_and_unsigned_ordered_comparisons_select_distinct_clif_conditions() {
    let cases = [
        (BinaryOp::Less, "slt", "ult"),
        (BinaryOp::LessEq, "sle", "ule"),
        (BinaryOp::Greater, "sgt", "ugt"),
        (BinaryOp::GreaterEq, "sge", "uge"),
    ];

    for (index, (op, signed_cc, unsigned_cc)) in cases.into_iter().enumerate() {
        let signed = binary_function(
            DefId((index * 2) as u32),
            int_ty(true, IntWidth::W64),
            op,
            None,
            Ty::Bool,
        );
        let unsigned = binary_function(
            DefId((index * 2 + 1) as u32),
            int_ty(false, IntWidth::W64),
            op,
            None,
            Ty::Bool,
        );

        let signed_clif = lower_single(
            signed,
            CraneliftBackend::aarch64().expect("AArch64 backend"),
        )
        .expect("signed comparison");
        let unsigned_clif = lower_single(
            unsigned,
            CraneliftBackend::aarch64().expect("AArch64 backend"),
        )
        .expect("unsigned comparison");

        assert!(
            signed_clif.contains(&format!("icmp {signed_cc}")),
            "{signed_clif}"
        );
        assert!(
            unsigned_clif.contains(&format!("icmp {unsigned_cc}")),
            "{unsigned_clif}"
        );
    }
}

#[test]
fn equality_comparisons_are_signedness_independent() {
    for op in [BinaryOp::Eq, BinaryOp::NotEq] {
        for signed in [false, true] {
            let function =
                binary_function(DefId(0), int_ty(signed, IntWidth::W32), op, None, Ty::Bool);
            let clif = lower_single(
                function,
                CraneliftBackend::riscv64().expect("RISC-V64 backend"),
            )
            .expect("equality comparison");
            let expected = if op == BinaryOp::Eq {
                "icmp eq"
            } else {
                "icmp ne"
            };
            assert!(clif.contains(expected), "{clif}");
        }
    }
}

#[test]
fn checked_add_lowers_to_overflow_test_and_trap() {
    let ty = int_ty(false, IntWidth::W64);
    let function = binary_function(
        DefId(0),
        ty.clone(),
        BinaryOp::Add,
        Some(OverflowMode::Checked),
        ty,
    );
    let clif = lower_single(
        function,
        CraneliftBackend::aarch64().expect("AArch64 backend"),
    )
    .expect("checked overflow should lower mechanically");

    assert!(clif.contains("uadd_overflow"), "{clif}");
    assert!(
        clif.contains("trapnz") && clif.contains("int_ovf"),
        "{clif}"
    );
}

// Backends are cheap to reconstruct; this helper keeps the nested test loop readable.
trait CloneForTest {
    fn clone_for_test(&self) -> CraneliftBackend;
}

impl CloneForTest for CraneliftBackend {
    fn clone_for_test(&self) -> CraneliftBackend {
        match self.target() {
            forge_codegen_cranelift::CraneliftTarget::Aarch64 => {
                CraneliftBackend::aarch64().expect("AArch64 backend")
            }
            forge_codegen_cranelift::CraneliftTarget::Riscv64 => {
                CraneliftBackend::riscv64().expect("RISC-V64 backend")
            }
        }
    }
}
