use forge_frontend::{
    body_hir::{HirExprKind, HirMatchBody, HirPatternKind, HirStmtKind, HirTypeKind, HirTypeRef},
    lower_module, lower_resolved_bodies, parse_source, DefId, LocalId, ResolvedName,
};

fn lower(source: &str) -> forge_frontend::BodyHirOutput {
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.expect("AST");
    let items = lower_module(&ast);
    assert!(items.diagnostics.is_empty(), "{:?}", items.diagnostics);
    lower_resolved_bodies(&ast, &items.module)
}

#[test]
fn embeds_local_ids_in_expression_uses() {
    let output = lower(
        r#"
        module test.hir_local;
        fn main(x: i32) -> i32 {
            val y: i32 = x;
            return y;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(0)];
    assert_eq!(body.params[0].0, LocalId(0));
    assert_eq!(body.locals[1].id, LocalId(1));

    let HirStmtKind::Value { value, .. } = &body.block.statements[0].kind else {
        panic!("expected value statement");
    };
    let HirExprKind::Name { reference } = &value.kind else {
        panic!("expected resolved name");
    };
    assert_eq!(reference.root, ResolvedName::Local(LocalId(0)));

    let HirStmtKind::Return {
        value: Some(value), ..
    } = &body.block.statements[1].kind
    else {
        panic!("expected return");
    };
    let HirExprKind::Name { reference } = &value.kind else {
        panic!("expected resolved name");
    };
    assert_eq!(reference.root, ResolvedName::Local(LocalId(1)));
}

#[test]
fn embeds_top_level_definition_ids() {
    let output = lower(
        r#"
        module test.hir_defs;
        fn first() -> i32 { return second(); }
        fn second() -> i32 { return 2; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(0)];
    let HirStmtKind::Return {
        value: Some(value), ..
    } = &body.block.statements[0].kind
    else {
        panic!("expected return");
    };
    let HirExprKind::Call { callee, .. } = &value.kind else {
        panic!("expected call");
    };
    let HirExprKind::Name { reference } = &callee.kind else {
        panic!("expected resolved callee");
    };
    assert_eq!(reference.root, ResolvedName::Def(DefId(1)));
}

#[test]
fn preserves_import_root_without_later_string_lookup() {
    let output = lower(
        r#"
        module test.hir_import;
        import std.io;
        fn main() -> i32 {
            io.println("hello");
            return 0;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(0)];
    let HirStmtKind::Expr { expr } = &body.block.statements[0].kind else {
        panic!("expected expression statement");
    };
    let HirExprKind::Call { callee, .. } = &expr.kind else {
        panic!("expected call");
    };
    let HirExprKind::Member { base, name } = &callee.kind else {
        panic!("expected member access");
    };
    assert_eq!(name, "println");
    let HirExprKind::Name { reference } = &base.kind else {
        panic!("expected import root");
    };
    assert_eq!(reference.root, ResolvedName::Import(0));
}

#[test]
fn resolves_user_and_builtin_types_in_hir() {
    let output = lower(
        r#"
        module test.hir_types;
        struct Point { x: i32; }
        fn get(p: Point) -> i32 { return p.x; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(1)];
    let HirTypeKind::Named { reference } = &body.params[0].1.kind else {
        panic!("expected named parameter type");
    };
    assert_eq!(reference, &HirTypeRef::Def(DefId(0)));
    let HirTypeKind::Named { reference } = &body.return_type.as_ref().expect("return type").kind
    else {
        panic!("expected named return type");
    };
    assert!(matches!(reference, HirTypeRef::Builtin { name } if name == "i32"));
}

#[test]
fn closure_capture_records_outer_source_and_inner_local() {
    let output = lower(
        r#"
        module test.hir_capture;
        fn main(x: i32) -> i32 {
            val add = [x](y: i32) -> i32 { return x + y; };
            return add(1);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(0)];
    let HirStmtKind::Value { value, .. } = &body.block.statements[0].kind else {
        panic!("expected closure binding");
    };
    let HirExprKind::Closure {
        captures, params, ..
    } = &value.kind
    else {
        panic!("expected closure");
    };
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].source, ResolvedName::Local(LocalId(0)));
    assert_eq!(captures[0].local, LocalId(1));
    assert_eq!(params[0].0, LocalId(2));
}

#[test]
fn unresolved_names_become_explicit_error_references() {
    let output = lower(
        r#"
        module test.hir_error;
        fn main() -> i32 { return missing; }
        "#,
    );
    assert_eq!(output.diagnostics.len(), 1);
    let body = &output.functions[&DefId(0)];
    let HirStmtKind::Return {
        value: Some(value), ..
    } = &body.block.statements[0].kind
    else {
        panic!("expected return");
    };
    let HirExprKind::Name { reference } = &value.kind else {
        panic!("expected name");
    };
    assert_eq!(reference.root, ResolvedName::Error);
}

#[test]
fn or_pattern_alternatives_share_canonical_local_ids() {
    let output = lower(
        r#"
        module test.hir_or_pattern;
        tagged Choice {
            Left { value: i32; },
            Right { value: i32; }
        }
        fn read(choice: Choice) -> i32 {
            return match (choice) {
                Choice::Left{value} | Choice::Right{value} => value,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.functions[&DefId(1)];
    let HirStmtKind::Return {
        value: Some(value), ..
    } = &body.block.statements[0].kind
    else {
        panic!("expected return");
    };
    let HirExprKind::Match { arms, .. } = &value.kind else {
        panic!("expected match");
    };
    let HirPatternKind::Or { patterns } = &arms[0].pattern.kind else {
        panic!("expected OR pattern");
    };
    let variant_local = |pattern: &forge_frontend::HirPattern| {
        let HirPatternKind::Variant { fields, .. } = &pattern.kind else {
            panic!("expected variant");
        };
        fields[0].shorthand_local.expect("value binding")
    };
    let left = variant_local(&patterns[0]);
    let right = variant_local(&patterns[1]);
    assert_eq!(
        left, right,
        "OR alternatives must reuse one logical LocalId"
    );

    let arm_expr = match &arms[0].body {
        HirMatchBody::Expr(expr) => expr,
        _ => panic!("expected expression arm"),
    };
    let HirExprKind::Name { reference } = &arm_expr.kind else {
        panic!("expected name use");
    };
    assert_eq!(reference.root, ResolvedName::Local(left));
}
