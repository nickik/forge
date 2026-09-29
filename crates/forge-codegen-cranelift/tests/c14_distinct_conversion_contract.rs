use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinition, TypeDefinitionKind, TypeDefinitionTable, TypeVariantDefinition,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn definitions() -> TypeDefinitionTable {
    BTreeMap::from([
        (
            DefId(100),
            TypeDefinition {
                owner: DefId(100),
                kind: TypeDefinitionKind::Distinct {
                    underlying: u(IntWidth::W32),
                },
            },
        ),
        (
            DefId(101),
            TypeDefinition {
                owner: DefId(101),
                kind: TypeDefinitionKind::Struct { fields: Vec::new() },
            },
        ),
        (
            DefId(102),
            TypeDefinition {
                owner: DefId(102),
                kind: TypeDefinitionKind::Tagged {
                    variants: vec![TypeVariantDefinition {
                        name: "Empty".into(),
                        declaration_index: 0,
                        fields: Vec::new(),
                    }],
                },
            },
        ),
        (
            DefId(103),
            TypeDefinition {
                owner: DefId(103),
                kind: TypeDefinitionKind::Enum {
                    variants: vec![TypeVariantDefinition {
                        name: "Ready".into(),
                        declaration_index: 0,
                        fields: Vec::new(),
                    }],
                },
            },
        ),
        (
            DefId(104),
            TypeDefinition {
                owner: DefId(104),
                kind: TypeDefinitionKind::BitStruct {
                    storage: u(IntWidth::W16),
                },
            },
        ),
    ])
}

fn function(source_ty: Ty, result_ty: Ty, instruction: FirInstructionKind) -> FirFunction {
    let local = FirLocalId(0);
    let source = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![local],
        return_type: result_ty.clone(),
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
                    span,
                    result: Some(source),
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
        value_types: BTreeMap::from([(source, source_ty), (result, result_ty)]),
    }
}

fn assert_invalid_fir(function: FirFunction) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions()) {
            Ok(_) => panic!("malformed distinct conversion unexpectedly verified"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFir {
                diagnostic_count: 1,
            }
        );
    }
}

fn comparison_function(owner: DefId) -> FirFunction {
    let ty = Ty::Nominal(owner);
    let left_local = FirLocalId(0);
    let right_local = FirLocalId(1);
    let left = FirValueId(0);
    let right = FirValueId(1);
    let result = FirValueId(2);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![left_local, right_local],
        return_type: Ty::Bool,
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
                        op: BinaryOp::Eq,
                        overflow: None,
                        left,
                        right,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(left, ty.clone()), (right, ty), (result, Ty::Bool)]),
    }
}

#[test]
fn distinct_wrap_requires_the_declared_underlying_type() {
    assert_invalid_fir(function(
        u(IntWidth::W16),
        Ty::Nominal(DefId(100)),
        FirInstructionKind::DistinctFromUnderlying {
            value: FirValueId(0),
            distinct: DefId(100),
        },
    ));
}

#[test]
fn distinct_unwrap_requires_the_declared_underlying_result() {
    assert_invalid_fir(function(
        Ty::Nominal(DefId(100)),
        u(IntWidth::W16),
        FirInstructionKind::DistinctToUnderlying {
            value: FirValueId(0),
            distinct: DefId(100),
        },
    ));
}

#[test]
fn distinct_comparison_requires_explicit_underlying_conversion() {
    assert_invalid_fir(comparison_function(DefId(100)));
}

#[test]
fn aggregate_comparisons_are_invalid_producer_contracts() {
    for owner in [DefId(101), DefId(102)] {
        assert_invalid_fir(comparison_function(owner));
    }
}

#[test]
fn enum_comparisons_are_invalid_producer_contracts() {
    assert_invalid_fir(comparison_function(DefId(103)));
}

#[test]
fn bitstruct_comparisons_are_invalid_producer_contracts() {
    assert_invalid_fir(comparison_function(DefId(104)));
}
