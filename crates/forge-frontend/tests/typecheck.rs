use forge_frontend::{
    lower_module, lower_resolved_bodies, parse_source, type_check_module, IntWidth, Ty,
};

fn check(source: &str) -> forge_frontend::TypeCheckOutput {
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "parse: {:?}", parsed.diagnostics);
    let ast = parsed.ast.expect("AST");
    let items = lower_module(&ast);
    assert!(items.diagnostics.is_empty(), "items: {:?}", items.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &items.module);
    assert!(bodies.diagnostics.is_empty(), "HIR: {:?}", bodies.diagnostics);
    type_check_module(&ast, &items.module, &bodies)
}

fn has(output: &forge_frontend::TypeCheckOutput, code: &str) -> bool {
    output.diagnostics.iter().any(|d| d.code == code)
}

#[test]
fn contextual_integer_literal_gets_declared_type() {
    let output = check(r#"
        module test.contextual_int;
        fn main() -> i32 {
            val x: u32 = 1;
            return 0;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|t| *t == Ty::Int { signed: false, width: IntWidth::W32 }));
}

#[test]
fn rejects_mixed_integer_width_arithmetic() {
    let output = check(r#"
        module test.mixed_width;
        fn main() -> i32 {
            val a: u8 = 1u8;
            val b: u32 = 2u32;
            val c = a + b;
            return 0;
        }
    "#);
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_signed_unsigned_comparison() {
    let output = check(r#"
        module test.signed_unsigned;
        fn main() -> i32 {
            val a: i32 = -1;
            val b: u32 = 1u32;
            val c: bool = a < b;
            return 0;
        }
    "#);
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_bool_integer_arithmetic() {
    let output = check(r#"
        module test.bool_integer;
        fn main() -> i32 {
            val enabled: bool = true;
            val bad = enabled + 1;
            return 0;
        }
    "#);
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_wrong_return_type() {
    let output = check(r#"
        module test.return_type;
        fn bad() -> i32 { return true; }
    "#);
    assert!(has(&output, "type/return"), "{:?}", output.diagnostics);
}

#[test]
fn supports_optional_promotion() {
    let output = check(r#"
        module test.optional_promote;
        fn main() -> i32 {
            val x: u32? = 1u32;
            return 0;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn explicit_numeric_conversion_is_valid() {
    let output = check(r#"
        module test.explicit_numeric;
        fn main() -> i32 {
            val a: u8 = 1u8;
            val b: u32 = u32(a);
            return 0;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn distinct_types_do_not_implicitly_mix() {
    let output = check(r#"
        module test.distinct_mix;
        distinct UserId: u32;
        distinct AccountId: u32;
        fn use_account(id: AccountId) -> u32 { return u32(id); }
        fn main() -> i32 {
            val user: UserId = UserId(7u32);
            val bad: u32 = use_account(user);
            return 0;
        }
    "#);
    assert!(has(&output, "type/mismatch") || has(&output, "type/distinct"), "{:?}", output.diagnostics);
}

#[test]
fn reports_duplicate_named_argument() {
    let output = check(r#"
        module test.named_duplicate;
        nfn connect(host: str, port: u16 = 80u16) -> bool { return true; }
        fn main() -> i32 {
            val ok = connect(:host = "a", :host = "b");
            return 0;
        }
    "#);
    assert!(has(&output, "call/duplicate-name"), "{:?}", output.diagnostics);
}

#[test]
fn reports_unknown_named_argument() {
    let output = check(r#"
        module test.named_unknown;
        nfn connect(host: str, port: u16 = 80u16) -> bool { return true; }
        fn main() -> i32 {
            val ok = connect(:host = "a", :bogus = 1u16);
            return 0;
        }
    "#);
    assert!(has(&output, "call/unknown-name"), "{:?}", output.diagnostics);
}

#[test]
fn return_tail_requires_call() {
    let output = check(r#"
        module test.tail_noncall;
        fn main() -> i32 { return tail 1; }
    "#);
    assert!(has(&output, "control/tail-call-required"), "{:?}", output.diagnostics);
}
