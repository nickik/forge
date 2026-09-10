use forge_frontend::{
    ast::{DeclKind, ExprKind, StmtKind, UnaryOp},
    parse_source,
};

fn first_function(source: &str) -> forge_frontend::ast::FunctionDecl {
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = parsed.ast.expect("AST");
    let DeclKind::Function(function) = &file.declarations[0].kind.kind else {
        panic!("expected function")
    };
    function.clone()
}

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
    let DeclKind::Function(main) = &file.declarations[0].kind.kind else {
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

#[test]
fn match_is_an_expression() {
    let function = first_function(
        r#"
        module test.match_expression;
        fn choose(x: i32) -> i32 {
            val y: i32 = match (x) {
                0 => 10,
                _ => 20,
            };
            return y;
        }
        "#,
    );

    let StmtKind::Value(value) = &function.body.statements[0].kind else {
        panic!("expected value declaration")
    };
    assert!(matches!(value.value.kind, ExprKind::Match { .. }));
}

#[test]
fn with_context_contains_named_forge_expressions() {
    let function = first_function(
        r#"
        module test.context_usage;
        fn main() -> i32 {
            with context(:scratch = &scratch, :logger = &logger) {
                run();
            }
            return 0;
        }
        "#,
    );

    let StmtKind::WithContext { overrides, .. } = &function.body.statements[0].kind else {
        panic!("expected with context")
    };
    assert_eq!(overrides.len(), 2);
    assert_eq!(overrides[0].name, "scratch");
    assert!(matches!(overrides[0].value.kind, ExprKind::Unary { .. }));
}

#[test]
fn assignment_is_a_statement() {
    let function = first_function(
        r#"
        module test.assignment;
        fn main() -> i32 {
            var x: i32 = 1;
            x = x + 1;
            return x;
        }
        "#,
    );
    assert!(matches!(
        function.body.statements[1].kind,
        StmtKind::Assignment { .. }
    ));
}

#[test]
fn method_call_is_member_then_call() {
    let function = first_function(
        r#"
        module test.method;
        fn main() -> i32 {
            point.length();
            return 0;
        }
        "#,
    );
    let StmtKind::Expr { expr } = &function.body.statements[0].kind else {
        panic!("expected expression statement")
    };
    let ExprKind::Call { callee, .. } = &expr.kind else {
        panic!("expected call")
    };
    assert!(matches!(callee.kind, ExprKind::Member { .. }));
}

#[test]
fn capture_free_closure_omits_capture_list() {
    let function = first_function(
        r#"
        module test.lambda;
        fn main() -> i32 {
            val inc = (x: i32) -> i32 { return x + 1; };
            return inc(1);
        }
        "#,
    );
    let StmtKind::Value(value) = &function.body.statements[0].kind else {
        panic!("expected closure binding")
    };
    let ExprKind::Closure { captures, .. } = &value.value.kind else {
        panic!("expected closure")
    };
    assert!(captures.is_empty());
}

#[test]
fn rejects_assignment_expression() {
    let parsed = parse_source(
        r#"
        module test.bad_assignment;
        fn main() -> i32 {
            var x: i32 = 0;
            val y: i32 = (x = 4);
            return y;
        }
        "#,
    );
    assert!(!parsed.diagnostics.is_empty() || parsed.ast.is_none());
}

#[test]
fn mutable_address_of_has_distinct_ast_operation() {
    let function = first_function(
        r#"
        module test.mutable_reference_expression;
        fn main() -> i32 {
            val pointer = &mut value;
            return 0;
        }
        "#,
    );
    let StmtKind::Value(value) = &function.body.statements[0].kind else {
        panic!("expected value declaration")
    };
    let ExprKind::Unary { op, .. } = value.value.kind else {
        panic!("expected unary expression")
    };
    assert_eq!(op, UnaryOp::AddressOfMut);
}

#[test]
fn fdn_reader_tag_can_use_forge_keyword() {
    let function = first_function(
        r#"
        module test.fdn_keyword_reader;
        fn main() -> i32 {
            val descriptor = #type "u32";
            return 0;
        }
        "#,
    );
    let StmtKind::Value(value) = &function.body.statements[0].kind else {
        panic!("expected value declaration")
    };
    let ExprKind::ReaderForm { tag, .. } = &value.value.kind else {
        panic!("expected reader form")
    };
    assert_eq!(tag, "type");
}

#[test]
fn return_tail_requires_expression() {
    let parsed = parse_source(
        r#"
        module test.bad_tail_return;
        fn main() -> i32 {
            return tail;
        }
        "#,
    );
    assert!(!parsed.diagnostics.is_empty() || parsed.ast.is_none());
}

#[test]
fn empty_enum_and_tagged_are_rejected() {
    for source in [
        "module test.empty_enum; enum Empty { }",
        "module test.empty_tagged; tagged Empty { }",
    ] {
        let parsed = parse_source(source);
        assert!(!parsed.diagnostics.is_empty(), "{source}");
    }
}
