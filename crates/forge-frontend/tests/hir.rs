use forge_frontend::{
    hir::{DefId, HirItemKind, MetadataTarget},
    lower_module, parse_source,
};

fn lower(source: &str) -> forge_frontend::hir::HirOutput {
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    lower_module(&parsed.ast.expect("AST"))
}

#[test]
fn assigns_definition_ids_in_source_order() {
    let output = lower(
        r#"
        module test.ids;
        struct Point { x: i32; }
        fn make() -> Point { return Point{x: 1}; }
        val origin: Point = Point{x: 0};
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.module.items.len(), 3);
    assert_eq!(output.module.items[0].id, DefId(0));
    assert_eq!(output.module.items[1].id, DefId(1));
    assert_eq!(output.module.items[2].id, DefId(2));
}

#[test]
fn keeps_type_and_value_namespaces_separate() {
    let output = lower(
        r#"
        module test.namespaces;
        struct Node { value: i32; }
        fn Node() -> i32 { return 1; }
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let symbol = output.module.symbols.get("Node").expect("Node symbol");
    assert_eq!(symbol.type_def, Some(DefId(0)));
    assert_eq!(symbol.value_def, Some(DefId(1)));
}

#[test]
fn reports_duplicate_definitions_in_same_namespace() {
    let output = lower(
        r#"
        module test.duplicates;
        fn work() -> i32 { return 1; }
        fn work() -> i32 { return 2; }
        struct Item { value: i32; }
        enum Item { One }
        "#,
    );

    assert_eq!(output.diagnostics.len(), 2);
    assert!(output.diagnostics[0]
        .message
        .contains("duplicate Value definition `work`"));
    assert!(output.diagnostics[1]
        .message
        .contains("duplicate Type definition `Item`"));
}

#[test]
fn collects_global_destructuring_bindings() {
    let output = lower(
        r#"
        module test.globals;
        val [first, second, ..rest] = values;
        fn main() -> i32 { return 0; }
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let HirItemKind::Global { bindings } = &output.module.items[0].kind else {
        panic!("expected global")
    };
    assert_eq!(bindings, &["first", "second", "rest"]);
    assert_eq!(output.module.symbols["first"].value_def, Some(DefId(0)));
    assert_eq!(output.module.symbols["second"].value_def, Some(DefId(0)));
    assert_eq!(output.module.symbols["rest"].value_def, Some(DefId(0)));
}

#[test]
fn records_impl_without_defining_target_again() {
    let output = lower(
        r#"
        module test.impls;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
        }
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.module.symbols["Point"].type_def, Some(DefId(0)));
    assert!(matches!(
        output.module.items[1].kind,
        HirItemKind::Impl { .. }
    ));
}

#[test]
fn preserves_metadata_by_semantic_target() {
    let output = lower(
        r#"
        module test.metadata_hir;

        @repr(c)
        struct Header {
            @align(4)
            word: u32;
        }

        @overflow(wrap)
        fn hash(x: u32) -> u32 { return x + 1u32; }

        impl Header {
            @inline
            fn get(self: &Header) -> u32 { return self.word; }
        }
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let item = output
        .module
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(0) })
        .expect("struct metadata");
    assert!(item.iter().any(|m| m.name.as_deref() == Some("repr")));

    let field = output
        .module
        .metadata
        .get(&MetadataTarget::Field {
            owner: DefId(0),
            variant: None,
            name: "word".into(),
        })
        .expect("field metadata");
    assert!(field.iter().any(|m| m.name.as_deref() == Some("align")));

    let function = output
        .module
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(1) })
        .expect("function metadata");
    assert!(function
        .iter()
        .any(|m| m.name.as_deref() == Some("overflow")));

    let method = output
        .module
        .metadata
        .get(&MetadataTarget::ImplMethod {
            owner: DefId(2),
            name: "get".into(),
        })
        .expect("impl method metadata");
    assert!(method.iter().any(|m| m.name.as_deref() == Some("inline")));
}
