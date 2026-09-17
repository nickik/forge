use forge_frontend::{
    lower_module, lower_resolved_bodies, parse_source, type_check_module, DefId, IntWidth,
    MetadataTarget, Ty,
};

fn check(source: &str) -> forge_frontend::TypeCheckOutput {
    let parsed = parse_source(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("AST");
    let items = lower_module(&ast);
    assert!(
        items.diagnostics.is_empty(),
        "items: {:?}",
        items.diagnostics
    );
    let bodies = lower_resolved_bodies(&ast, &items.module);
    assert!(
        bodies.diagnostics.is_empty(),
        "HIR: {:?}",
        bodies.diagnostics
    );
    type_check_module(&ast, &items.module, &bodies)
}

fn has(output: &forge_frontend::TypeCheckOutput, code: &str) -> bool {
    output.diagnostics.iter().any(|d| d.code == code)
}

#[test]
fn immutable_globals_reject_assignment_and_mutable_address() {
    let output = check(
        r#"
        module test.immutable_globals;
        val READY: i32 = 1i32;
        fn main() -> i32 {
            READY = 2i32;
            val address = &mut READY;
            return READY;
        }
        "#,
    );
    assert!(
        has(&output, "assignment/immutable"),
        "{:?}",
        output.diagnostics
    );
    assert!(
        has(&output, "reference/immutable"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn contextual_integer_literal_gets_declared_type() {
    let output = check(
        r#"
        module test.contextual_int;
        fn main() -> i32 {
            val x: u32 = 1;
            return 0;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|t| *t
        == Ty::Int {
            signed: false,
            width: IntWidth::W32
        }));
}

#[test]
fn rejects_mixed_integer_width_arithmetic() {
    let output = check(
        r#"
        module test.mixed_width;
        fn main() -> i32 {
            val a: u8 = 1u8;
            val b: u32 = 2u32;
            val c = a + b;
            return 0;
        }
    "#,
    );
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_signed_unsigned_comparison() {
    let output = check(
        r#"
        module test.signed_unsigned;
        fn main() -> i32 {
            val a: i32 = -1;
            val b: u32 = 1u32;
            val c: bool = a < b;
            return 0;
        }
    "#,
    );
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_bool_integer_arithmetic() {
    let output = check(
        r#"
        module test.bool_integer;
        fn main() -> i32 {
            val enabled: bool = true;
            val bad = enabled + 1;
            return 0;
        }
    "#,
