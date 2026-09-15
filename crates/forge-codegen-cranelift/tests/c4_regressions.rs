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

fn lower_error(function: FirFunction) -> BackendError {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    match CraneliftBackend::aarch64()
        .expect("AArch64 backend")
        .prepare_module(&module)
    {
        Ok(_) => panic!("malformed C4 FIR unexpectedly lowered"),
        Err(error) => error,
    }
}

fn one_param_function(
    local_ty: Ty,
    load_ty: Ty,
    result_ty: Ty,
    instruction: FirInstructionKind,
) -> FirFunction {
    let owner = DefId(30);
    let local = FirLocalId(0);
    let loaded = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);

    FirFunction {
        owner,
        params: vec![local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: local_ty,
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
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: instruction,
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(loaded, load_ty), (result, result_ty)]),
    }
}

#[test]
fn load_rejects_equal_width_but_different_fir_type() {
    let u8_ty = int_ty(false, IntWidth::W8);
    let i8_ty = int_ty(true, IntWidth::W8);
    let function = one_param_function(
        u8_ty,
        i8_ty.clone(),
        i8_ty,
        FirInstructionKind::Unary {
            op: FirUnaryOp::BitNot,
            value: FirValueId(0),
        },
    );

    assert!(matches!(
        lower_error(function),
        BackendError::InvalidFirShape { .. }
    ));
}

#[test]
fn unary_rejects_signedness_change_hidden_by_same_clif_width() {
    let u8_ty = int_ty(false, IntWidth::W8);
    let i8_ty = int_ty(true, IntWidth::W8);
    let function = one_param_function(
        u8_ty.clone(),
        u8_ty,
        i8_ty,
        FirInstructionKind::Unary {
            op: FirUnaryOp::BitNot,
            value: FirValueId(0),
        },
    );

    assert!(matches!(
        lower_error(function),
        BackendError::InvalidFirShape { .. }
    ));
}

fn malformed_binary(result_ty: Ty, op: BinaryOp) -> FirFunction {
    let owner = DefId(31);
    let ty = int_ty(false, IntWidth::W8);
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
                        overflow: if matches!(op, BinaryOp::Add) {
                            Some(OverflowMode::Wrapping)
                        } else {
                            None
                        },
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

#[test]
fn comparison_result_must_be_exactly_bool() {
    let function = malformed_binary(int_ty(true, IntWidth::W8), BinaryOp::Eq);
    assert!(matches!(
        lower_error(function),
        BackendError::InvalidFirShape { .. }
    ));
}

#[test]
fn arithmetic_result_must_match_operand_fir_type() {
    let function = malformed_binary(int_ty(true, IntWidth::W8), BinaryOp::Add);
    assert!(matches!(
        lower_error(function),
        BackendError::InvalidFirShape { .. }
    ));
}

#[test]
fn dependency_order_need_not_match_block_id_order() {
    let owner = DefId(32);
    let ty = int_ty(false, IntWidth::W64);
    let value = FirValueId(0);
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
                    target: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![],
                terminator: Some(FirTerminator::Return { value: Some(value) }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(value),
                    kind: FirInstructionKind::Const {
                        value: forge_fir::FirConst::Integer { text: "42".into() },
                    },
                }],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(1),
                }),
            },
        ],
        value_types: BTreeMap::from([(value, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    let prepared = CraneliftBackend::aarch64()
        .expect("AArch64 backend")
        .prepare_module(&module)
        .expect("C4 must preserve C3 dependency scheduling");
    let clif = prepared
        .function(owner)
        .expect("lowered function")
        .display()
        .to_string();
    assert!(clif.contains("iconst.i64 42"), "{clif}");
    assert!(clif.contains("return"), "{clif}");
}
