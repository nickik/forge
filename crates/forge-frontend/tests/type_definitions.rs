use forge_frontend::{
    collect_type_definitions, lower_module, lower_resolved_bodies, parse_source, type_check_module,
    DefId, IntWidth, Ty, TypeDefinitionKind,
};

#[test]
fn resolved_type_definitions_preserve_source_identity_and_order() {
    let parsed = parse_source(
        r#"
        module test.layout_types;
        struct Record {
            small: u8;
            wide: u64;
            middle: u16;
        }
        tagged Value {
            Empty,
            Pair { left: u8; right: u32; },
        }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("AST");
    let hir = lower_module(&ast);
    assert!(
        hir.diagnostics.is_empty(),
        "HIR items: {:?}",
        hir.diagnostics
    );
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(
        bodies.diagnostics.is_empty(),
        "HIR bodies: {:?}",
        bodies.diagnostics
    );
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(
        typed.diagnostics.is_empty(),
        "typed: {:?}",
        typed.diagnostics
    );

    let definitions = collect_type_definitions(&ast, &hir.module, &bodies, &typed);

    let record = definitions.get(&DefId(0)).expect("Record definition");
    let TypeDefinitionKind::Struct { fields } = &record.kind else {
        panic!("Record should be a struct");
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| (field.name.as_str(), field.declaration_index))
            .collect::<Vec<_>>(),
        vec![("small", 0), ("wide", 1), ("middle", 2)]
    );
    assert_eq!(
        fields[1].ty,
        Ty::Int {
            signed: false,
            width: IntWidth::W64,
        }
    );

    let tagged = definitions.get(&DefId(1)).expect("Value definition");
    let TypeDefinitionKind::Tagged { variants } = &tagged.kind else {
        panic!("Value should be tagged");
    };
    assert_eq!(
        variants
            .iter()
            .map(|variant| (variant.name.as_str(), variant.declaration_index))
            .collect::<Vec<_>>(),
        vec![("Empty", 0), ("Pair", 1)]
    );
    assert!(variants[0].fields.is_empty());
    assert_eq!(
        variants[1]
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.declaration_index))
            .collect::<Vec<_>>(),
        vec![("left", 0), ("right", 1)]
    );
    assert_eq!(
        variants[1].fields[1].ty,
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        }
    );
}
