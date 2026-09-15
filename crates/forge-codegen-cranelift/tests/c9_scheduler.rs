use std::collections::BTreeMap;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty, TypeDefinition, TypeDefinitionKind,
    TypeDefinitionTable, TypeFieldDefinition,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn field(name: &str, index: u32, ty: Ty) -> TypeFieldDefinition {
    TypeFieldDefinition {
        name: name.into(),
        ty,
        declaration_index: index,
    }
}

fn definitions() -> TypeDefinitionTable {
    let owner = DefId(100);
    BTreeMap::from([(
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::Struct {
                fields: vec![
                    field("a", 0, u(IntWidth::W8)),
                    field("b", 1, u(IntWidth::W64)),
                    field("c", 2, u(IntWidth::W16)),
                    field("d", 3, u(IntWidth::W32)),
                ],
            },
        },
    )])
}

fn scalar_const(span: Span, result: FirValueId, text: &str) -> FirInstruction {
    FirInstruction {
        span,
        result: Some(result),
        kind: FirInstructionKind::Const {
            value: FirConst::Integer { text: text.into() },
        },
    }
}

#[test]
fn non_topological_blocks_schedule_aggregate_dependency() {
    let span = Span::new(0, 0);
    let record_ty = Ty::Nominal(DefId(100));
    let a = FirValueId(0);
    let b = FirValueId(1);
    let c = FirValueId(2);
    let d = FirValueId(3);
    let aggregate = FirValueId(4);
    let extracted = FirValueId(5);

    // FirBlockId remains the vector index. The valid CFG/value order is 0 -> 2 -> 1,
    // deliberately different from vector order, so aggregate results must participate
    // in the same dependency scheduling used by scalar C3/C7/C8 lowering.
    let function = FirFunction {
        owner: DefId(6),
        params: vec![],
        return_type: u(IntWidth::W64),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    scalar_const(span, a, "1"),
                    scalar_const(span, b, "99"),
                    scalar_const(span, c, "2"),
                    scalar_const(span, d, "3"),
                ],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(extracted),
                    kind: FirInstructionKind::ExtractField {
                        base: aggregate,
                        field: "b".into(),
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(extracted),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(aggregate),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: record_ty.clone(),
                        variant: None,
                        fields: vec![
                            ("a".into(), a),
                            ("b".into(), b),
                            ("c".into(), c),
                            ("d".into(), d),
                        ],
                    },
                }],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(1),
                }),
            },
        ],
        value_types: BTreeMap::from([
            (a, u(IntWidth::W8)),
            (b, u(IntWidth::W64)),
            (c, u(IntWidth::W16)),
            (d, u(IntWidth::W32)),
            (aggregate, record_ty),
            (extracted, u(IntWidth::W64)),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = definitions();

    for backend in [
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        CraneliftBackend::riscv64().expect("RISC-V64 backend"),
    ] {
        backend
            .prepare_module_with_types(&module, &definitions)
            .expect("non-topological aggregate dependency should schedule and verify");
    }
}
