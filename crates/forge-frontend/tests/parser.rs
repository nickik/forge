use forge_frontend::{
    ast::{DeclKind, StmtKind},
    parse_source,
};

#[test]
fn parses_hello_program() {
    let source = r#"
        module examples.hello;
        import std.io;

        fn main() -> i32 {
            val name: str = "Forge";
            io.println("Hello, {}", name);
            return 0;
        }
    "#;

    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = parsed.ast.expect("AST");
    assert_eq!(file.module.segments, ["examples", "hello"]);
    assert_eq!(file.imports[0].segments, ["std", "io"]);
    let DeclKind::Function(main) = &file.declarations[0].kind else {
        panic!("expected function")
    };
    assert_eq!(main.name, "main");
    assert!(matches!(main.body.statements[0].kind, StmtKind::Value(_)));
    assert!(matches!(
        main.body.statements[2].kind,
        StmtKind::Return { .. }
    ));
}

#[test]
fn respects_operator_precedence() {
    let source = r#"
        module test.math;
        fn main() -> i32 {
            return 1 + 2 * 3;
        }
    "#;
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(parsed.ast.is_some());
}

#[test]
fn parses_struct_and_optional_type() {
    let source = r#"
        module test.types;
        struct User {
            id: u32;
            manager: &User?;
        }
        fn main() -> i32 { return 0; }
    "#;
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(parsed.ast.is_some());
}
