use forge_frontend::{lower_module, parse_source, resolve_module_bodies, DefId, ResolvedName};

fn resolve(source: &str) -> forge_frontend::BodyResolutionOutput {
    let parsed = parse_source(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("AST");
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "items: {:?}", hir.diagnostics);
    resolve_module_bodies(&ast, &hir.module)
}

#[test]
fn resolves_parameter_and_shadowed_local() {
    let output = resolve(
        r#"
        module test.resolve_local;
        fn f(x: i32) -> i32 {
            val before: i32 = x;
            {
                val x: i32 = 7;
                val inside: i32 = x;
            }
            return x;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.bodies[&DefId(0)];
    assert_eq!(body.locals.iter().filter(|l| l.name == "x").count(), 2);
    let x_uses = body
        .uses
        .iter()
        .filter(|u| u.name == "x")
        .collect::<Vec<_>>();
    assert_eq!(x_uses.len(), 3);
    assert!(matches!(x_uses[0].resolution, ResolvedName::Local(_)));
    assert_ne!(x_uses[0].resolution, x_uses[1].resolution);
    assert_eq!(x_uses[0].resolution, x_uses[2].resolution);
}

#[test]
fn rejects_duplicate_local_in_same_scope() {
    let output = resolve(
        r#"
        module test.duplicate_local;
        fn f() -> i32 {
            val x: i32 = 1;
            val x: i32 = 2;
            return x;
        }
    "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.message.contains("duplicate local definition `x`")));
}

#[test]
fn resolves_forward_top_level_function() {
    let output = resolve(
        r#"
        module test.forward;
        fn first() -> i32 { return second(); }
        fn second() -> i32 { return 2; }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.bodies[&DefId(0)];
    let second = body
        .uses
        .iter()
        .find(|u| u.name == "second")
        .expect("second use");
    assert_eq!(second.resolution, ResolvedName::Def(DefId(1)));
}

#[test]
fn resolves_import_alias_on_member_access() {
    let output = resolve(
        r#"
        module test.imports;
        import std.io;
        fn f() -> i32 {
            io.println("x");
            return 0;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.bodies[&DefId(0)];
    let io = body.uses.iter().find(|u| u.name == "io").expect("io use");
    assert_eq!(io.resolution, ResolvedName::Import(0));
}

#[test]
fn reports_unresolved_value_and_type_names() {
    let output = resolve(
        r#"
        module test.unresolved;
        fn f(x: MissingType) -> i32 {
            return missing_value;
        }
    "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved type name `MissingType`")));
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.message.contains("unresolved value name `missing_value`")));
}

#[test]
fn pattern_bindings_enter_arm_scope() {
    let output = resolve(
        r#"
        module test.pattern_scope;
        fn f(value: i32?) -> i32 {
            return match (value) {
                Some(x) => x,
                None => 0,
            };
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.bodies[&DefId(0)];
    assert!(body.locals.iter().any(|l| l.name == "x"));
    assert!(body
        .uses
        .iter()
        .any(|u| u.name == "x" && matches!(u.resolution, ResolvedName::Local(_))));
}

#[test]
fn builtin_types_resolve_without_module_symbols() {
    let output = resolve(
        r#"
        module test.builtins;
        fn f(x: u32) -> i64 { return 0; }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = &output.bodies[&DefId(0)];
    assert!(body
        .uses
        .iter()
        .any(|u| u.name == "u32" && u.resolution == ResolvedName::BuiltinType));
    assert!(body
        .uses
        .iter()
        .any(|u| u.name == "i64" && u.resolution == ResolvedName::BuiltinType));
}

#[test]
fn bare_enum_variant_requires_qualification() {
    let parsed = forge_frontend::parse_source(
        r#"
        module test.bare_variant;
        enum Color { Red, Green, }
        fn main() -> i32 {
            val c: Color = Red;
            return 0;
        }
    "#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.expect("AST");
    let items = forge_frontend::lower_module(&ast);
    assert!(items.diagnostics.is_empty(), "{:?}", items.diagnostics);
    let resolved = forge_frontend::resolve_module_bodies(&ast, &items.module);
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("name/qualified-variant-required:")),
        "{:?}",
        resolved.diagnostics
    );
}
