use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinition, TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn definitions() -> TypeDefinitionTable {
    let owner = DefId(100);
    BTreeMap::from([(
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::Struct {
                fields: vec![TypeFieldDefinition {
                    name: "count".into(),
                    ty: u(IntWidth::W32),
                    declaration_index: 0,
                }],
            },
        },
    )])
}

fn function(base_ty: Ty, result_ty: Option<Ty>, field: &str) -> FirFunction {
    let local = FirLocalId(0);
    let base = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    let return_type = result_ty.clone().unwrap_or(Ty::Void);
    let mut value_types = BTreeMap::from([(base, base_ty.clone())]);
    if let Some(ty) = result_ty.clone() {
        value_types.insert(result, ty);
    }
    FirFunction {
        owner: DefId(1),
        params: vec![local],
        return_type,
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: base_ty,
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
                    result: Some(base),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                },
                FirInstruction {
                    span,
                    result: result_ty.as_ref().map(|_| result),
                    kind: FirInstructionKind::ExtractField {
                        base,
                        field: field.into(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: result_ty.as_ref().map(|_| result),
            }),
        }],
        value_types,
    }
}

fn assert_invalid(function: FirFunction, definitions: &TypeDefinitionTable, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, definitions) {
            Ok(_) => panic!("malformed extract-field FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: message.into(),
            }
        );
    }
}

#[test]
fn extract_field_requires_a_result() {
    assert_invalid(
        function(Ty::Nominal(DefId(100)), None, "count"),
        &definitions(),
        "extract-field has no result",
    );
}

#[test]
fn extract_field_requires_a_field_bearing_base() {
    assert_invalid(
        function(u(IntWidth::W8), Some(u(IntWidth::W32)), "count"),
        &TypeDefinitionTable::new(),
        "field access on non-nominal Int { signed: false, width: W8 }",
    );
}

#[test]
fn extract_field_requires_the_declared_result_type() {
    assert_invalid(
        function(Ty::Nominal(DefId(100)), Some(u(IntWidth::W16)), "count"),
        &definitions(),
        "extract-field result has FIR type Int { signed: false, width: W16 }, declared field `count` has type Int { signed: false, width: W32 }",
    );
}
