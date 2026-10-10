use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirUnaryOp, FirValueId, IntWidth,
    OverflowMode, Span, Ty,
};

fn int_ty(signed: bool, width: IntWidth) -> Ty {
    Ty::Int { signed, width }
}

fn prepare(
    function: FirFunction,
    backend: CraneliftBackend,
) -> Result<forge_codegen_cranelift::PreparedModule, BackendError> {
    let owner = function.owner;
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    backend.prepare_module(&module)
}

fn lower(function: FirFunction, backend: CraneliftBackend) -> String {
    let owner = function.owner;
    prepare(function, backend)
        .expect("C4b FIR should lower")
        .function(owner)
        .expect("lowered function")
        .display()
        .to_string()
}

fn binary(op: BinaryOp, ty: Ty, overflow: Option<OverflowMode>) -> FirFunction {
    let p0 = FirLocalId(0);
    let p1 = FirLocalId(1);
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let v2 = FirValueId(2);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(0),
        params: vec![p0, p1],
        return_type: ty.clone(),
        locals: BTreeMap::from([
            (
                p0,
                FirLocal {
                    id: p0,
                    source: None,
                    ty: ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                p1,
                FirLocal {
                    id: p1,
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
                    result: Some(v0),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: p0 },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v1),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: p1 },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v2),
                    kind: FirInstructionKind::Binary {
                        op,
                        overflow,
                        left: v0,
                        right: v1,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(v2) }),
        }],
        value_types: BTreeMap::from([(v0, ty.clone()), (v1, ty.clone()), (v2, ty)]),
    }
}

fn unary(op: FirUnaryOp, ty: Ty) -> FirFunction {
    let p0 = FirLocalId(0);
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(0),
        params: vec![p0],
        return_type: ty.clone(),
        locals: BTreeMap::from([(
            p0,
            FirLocal {
                id: p0,
                source: None,
                ty: ty.clone(),
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
                    span,
                    result: Some(v0),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: p0 },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v1),
                    kind: FirInstructionKind::Unary { op, value: v0 },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(v1) }),
        }],
        value_types: BTreeMap::from([(v0, ty.clone()), (v1, ty)]),
    }
}

fn conversion(source: Ty, target: Ty) -> FirFunction {
    let p0 = FirLocalId(0);
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(0),
        params: vec![p0],
        return_type: target.clone(),
        locals: BTreeMap::from([(
            p0,
            FirLocal {
                id: p0,
                source: None,
                ty: source.clone(),
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
                    span,
                    result: Some(v0),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: p0 },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v1),
                    kind: FirInstructionKind::Convert {
                        value: v0,
                        target: target.clone(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(v1) }),
        }],
        value_types: BTreeMap::from([(v0, source), (v1, target)]),
    }
}

#[test]
fn div_and_rem_select_signedness() {
    for backend in [
        CraneliftBackend::aarch64().expect("AArch64"),
        CraneliftBackend::riscv64().expect("RV64"),
    ] {
        for (signed, div, rem) in [(true, "sdiv", "srem"), (false, "udiv", "urem")] {
            let ty = int_ty(signed, IntWidth::W64);
            let div_clif = lower(
                binary(BinaryOp::Div, ty.clone(), Some(OverflowMode::Checked)),
                rebuild(&backend),
            );
            let rem_clif = lower(
                binary(BinaryOp::Rem, ty.clone(), Some(OverflowMode::Checked)),
                rebuild(&backend),
            );
            assert!(div_clif.contains(div), "{div_clif}");
            assert!(rem_clif.contains(rem), "{rem_clif}");
        }
    }
}

#[test]
fn bitwise_ops_lower_directly() {
    let ty = int_ty(false, IntWidth::W32);
    for (op, expected) in [
        (BinaryOp::BitAnd, "band"),
        (BinaryOp::BitXor, "bxor"),
        (BinaryOp::BitOr, "bor"),
    ] {
        let clif = lower(
            binary(op, ty.clone(), None),
            CraneliftBackend::aarch64().expect("AArch64"),
        );
        assert!(clif.contains(expected), "{clif}");
    }
}

#[test]
fn boolean_equality_uses_boolean_comparison_lowering() {
    for (op, condition) in [(BinaryOp::Eq, "icmp eq"), (BinaryOp::NotEq, "icmp ne")] {
        let clif = lower(
            binary(op, Ty::Bool, None),
            CraneliftBackend::aarch64().expect("AArch64"),
        );
        assert!(clif.contains(condition), "{clif}");
    }
}

#[test]
fn ordered_boolean_comparisons_remain_a_hard_backend_error() {
    let error = match prepare(
        binary(BinaryOp::Less, Ty::Bool, None),
        CraneliftBackend::aarch64().expect("AArch64"),
    ) {
        Ok(_) => panic!("ordered bool comparison is invalid FIR"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::UnsupportedInstruction {
            kind: "ordered boolean comparison"
        }
    );
}

#[test]
fn shifts_check_count_and_select_signed_right_shift() {
    let signed = int_ty(true, IntWidth::W32);
    let unsigned = int_ty(false, IntWidth::W32);
    let left = lower(
        binary(
            BinaryOp::ShiftLeft,
            signed.clone(),
            Some(OverflowMode::Checked),
        ),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    let signed_right = lower(
        binary(BinaryOp::ShiftRight, signed, Some(OverflowMode::Checked)),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    let unsigned_right = lower(
        binary(BinaryOp::ShiftRight, unsigned, Some(OverflowMode::Checked)),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    assert!(left.contains("ishl") && left.contains("trapnz"), "{left}");
    assert!(
        signed_right.contains("sshr") && signed_right.contains("trapnz"),
        "{signed_right}"
    );
    assert!(
        unsigned_right.contains("ushr") && unsigned_right.contains("trapnz"),
        "{unsigned_right}"
    );
}

#[test]
fn checked_add_sub_mul_emit_overflow_traps_for_signed_and_unsigned() {
    for signed in [false, true] {
        let ty = int_ty(signed, IntWidth::W32);
        for (op, stem) in [
            (BinaryOp::Add, "add_overflow"),
            (BinaryOp::Sub, "sub_overflow"),
            (BinaryOp::Mul, "mul_overflow"),
        ] {
            let clif = lower(
                binary(op, ty.clone(), Some(OverflowMode::Checked)),
                CraneliftBackend::riscv64().expect("RV64"),
            );
            if !signed && op == BinaryOp::Add {
                assert!(clif.contains(stem), "{clif}");
            }
            assert!(
                clif.contains("trapnz") && clif.contains("int_ovf"),
                "{clif}"
            );
        }
    }
}

#[test]
fn bitnot_lowers_but_negation_waits_for_explicit_fir_overflow_semantics() {
    let ty = int_ty(true, IntWidth::W64);
    let not = lower(
        unary(FirUnaryOp::BitNot, ty.clone()),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    assert!(not.contains("bnot"), "{not}");

    let error = match prepare(
        unary(FirUnaryOp::Neg, ty),
        CraneliftBackend::aarch64().expect("AArch64"),
    ) {
        Ok(_) => panic!("negation unexpectedly lowered without FIR overflow semantics"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::UnsupportedInstruction {
            kind: "integer negation requires explicit FIR overflow semantics"
        }
    );
}

#[test]
fn lossless_integer_widening_uses_sign_or_zero_extension() {
    let signed8 = int_ty(true, IntWidth::W8);
    let unsigned8 = int_ty(false, IntWidth::W8);
    let signed64 = int_ty(true, IntWidth::W64);
    let unsigned64 = int_ty(false, IntWidth::W64);

    let sext = lower(
        conversion(signed8, signed64.clone()),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    let uext = lower(
        conversion(unsigned8.clone(), unsigned64),
        CraneliftBackend::aarch64().expect("AArch64"),
    );
    let unsigned_to_wider_signed = lower(
        conversion(unsigned8, signed64),
        CraneliftBackend::aarch64().expect("AArch64"),
    );

    assert!(sext.contains("sextend"), "{sext}");
    assert!(uext.contains("uextend"), "{uext}");
    assert!(
        unsigned_to_wider_signed.contains("uextend"),
        "{unsigned_to_wider_signed}"
    );
}

#[test]
fn explicit_integer_conversions_range_check_when_the_source_domain_does_not_fit() {
    let cases = [
        (int_ty(false, IntWidth::W64), int_ty(false, IntWidth::W16)),
        (int_ty(false, IntWidth::W8), int_ty(true, IntWidth::W8)),
        (int_ty(true, IntWidth::W8), int_ty(false, IntWidth::W64)),
        (int_ty(false, IntWidth::W32), int_ty(true, IntWidth::W32)),
        (int_ty(true, IntWidth::W64), int_ty(true, IntWidth::W16)),
    ];

    for backend in [
        CraneliftBackend::aarch64().expect("AArch64"),
        CraneliftBackend::riscv64().expect("RV64"),
    ] {
        for (source, target) in cases.clone() {
            let clif = lower(conversion(source, target), rebuild(&backend));
            assert!(clif.contains("icmp"), "missing range comparison:\n{clif}");
            assert!(
                clif.contains("trapnz") && clif.contains("int_ovf"),
                "missing conversion trap:\n{clif}"
            );
        }
    }
}

#[test]
fn explicit_lossless_widening_does_not_add_a_range_trap() {
    for backend in [
        CraneliftBackend::aarch64().expect("AArch64"),
        CraneliftBackend::riscv64().expect("RV64"),
    ] {
        let clif = lower(
            conversion(int_ty(false, IntWidth::W8), int_ty(true, IntWidth::W64)),
            rebuild(&backend),
        );
        assert!(clif.contains("uextend"), "{clif}");
        assert!(!clif.contains("trapnz"), "unexpected range trap:\n{clif}");
    }
}

#[test]
fn integer_conversion_range_checks_cover_each_source_and_target_domain() {
    let types = [
        Ty::Byte,
        int_ty(false, IntWidth::W8),
        int_ty(true, IntWidth::W8),
        int_ty(false, IntWidth::W16),
        int_ty(true, IntWidth::W16),
        int_ty(false, IntWidth::W32),
        int_ty(true, IntWidth::W32),
        int_ty(false, IntWidth::W64),
        int_ty(true, IntWidth::W64),
        int_ty(false, IntWidth::Pointer),
        int_ty(true, IntWidth::Pointer),
    ];
    let width = |ty: &Ty| match ty {
        Ty::Byte => 8,
        Ty::Int {
            width: IntWidth::W8,
            ..
        } => 8,
        Ty::Int {
            width: IntWidth::W16,
            ..
        } => 16,
        Ty::Int {
            width: IntWidth::W32,
            ..
        } => 32,
        Ty::Int {
            width: IntWidth::W64,
            ..
        } => 64,
        Ty::Int {
            width: IntWidth::Pointer,
            ..
        } => 64,
        _ => unreachable!("test only uses integer types"),
    };
    let signed = |ty: &Ty| matches!(ty, Ty::Int { signed: true, .. });

    for backend in [
        CraneliftBackend::aarch64().expect("AArch64"),
        CraneliftBackend::riscv64().expect("RV64"),
    ] {
        for source in types.iter().cloned() {
            for target in types.iter().cloned() {
                let needs_check = if source == target {
                    false
                } else if signed(&source) {
                    signed(&target) && width(&target) < width(&source) || !signed(&target)
                } else if signed(&target) {
                    width(&target) <= width(&source)
                } else {
                    width(&target) < width(&source)
                };
                let clif = lower(
                    conversion(source.clone(), target.clone()),
                    rebuild(&backend),
                );
                assert_eq!(
                    clif.contains("trapnz"),
                    needs_check,
                    "wrong conversion range policy for {source:?} -> {target:?}:\n{clif}"
                );
            }
        }
    }
}

fn rebuild(backend: &CraneliftBackend) -> CraneliftBackend {
    match backend.target() {
        forge_codegen_cranelift::CraneliftTarget::Aarch64 => {
            CraneliftBackend::aarch64().expect("AArch64")
        }
        forge_codegen_cranelift::CraneliftTarget::Riscv64 => {
            CraneliftBackend::riscv64().expect("RV64")
        }
        forge_codegen_cranelift::CraneliftTarget::Sia32 => {
            panic!("SIA32 CLIF lowering is intentionally unavailable before M5")
        }
    }
}
