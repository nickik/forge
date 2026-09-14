use forge_frontend::{
    lower_module, lower_resolved_bodies, parse_source, type_check_module, IntWidth, Ty,
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
    );
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_wrong_return_type() {
    let output = check(
        r#"
        module test.return_type;
        fn bad() -> i32 { return true; }
    "#,
    );
    assert!(has(&output, "type/return"), "{:?}", output.diagnostics);
}

#[test]
fn supports_optional_promotion() {
    let output = check(
        r#"
        module test.optional_promote;
        fn main() -> i32 {
            val x: u32? = 1u32;
            return 0;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn explicit_numeric_conversion_is_valid() {
    let output = check(
        r#"
        module test.explicit_numeric;
        fn main() -> i32 {
            val a: u8 = 1u8;
            val b: u32 = u32(a);
            return 0;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn distinct_types_do_not_implicitly_mix() {
    let output = check(
        r#"
        module test.distinct_mix;
        distinct UserId: u32;
        distinct AccountId: u32;
        fn use_account(id: AccountId) -> u32 { return u32(id); }
        fn main() -> i32 {
            val user: UserId = UserId(7u32);
            val bad: u32 = use_account(user);
            return 0;
        }
    "#,
    );
    assert!(
        has(&output, "type/mismatch") || has(&output, "type/distinct"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn reports_duplicate_named_argument() {
    let output = check(
        r#"
        module test.named_duplicate;
        nfn connect(host: str, port: u16 = 80u16) -> bool { return true; }
        fn main() -> i32 {
            val ok = connect(:host = "a", :host = "b");
            return 0;
        }
    "#,
    );
    assert!(
        has(&output, "call/duplicate-name"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn reports_unknown_named_argument() {
    let output = check(
        r#"
        module test.named_unknown;
        nfn connect(host: str, port: u16 = 80u16) -> bool { return true; }
        fn main() -> i32 {
            val ok = connect(:host = "a", :bogus = 1u16);
            return 0;
        }
    "#,
    );
    assert!(
        has(&output, "call/unknown-name"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn return_tail_requires_call() {
    let output = check(
        r#"
        module test.tail_noncall;
        fn main() -> i32 { return tail 1; }
    "#,
    );
    assert!(
        has(&output, "control/tail-call-required"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn rejects_type_name_as_index_operand() {
    let output = check(
        r#"
        module test.type_index;
        struct Point { x: i32; }
        fn identity(x: i32) -> i32 { return x; }
        fn main() -> i32 {
            val value: i32 = 1;
            val x = identity[Point](value);
            return 0;
        }
    "#,
    );
    assert!(
        has(&output, "type/index-on-type"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn rejects_assignment_to_val() {
    let output = check(
        r#"
        module test.immutable_assignment;
        fn main() -> i32 {
            val x: i32 = 1;
            x = 2;
            return x;
        }
    "#,
    );
    assert!(
        has(&output, "assignment/immutable"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn allows_assignment_to_var() {
    let output = check(
        r#"
        module test.mutable_assignment;
        fn main() -> i32 {
            var x: i32 = 1;
            x = 2;
            return x;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn struct_member_access_has_exact_type() {
    let output = check(
        r#"
        module test.member_type;
        struct Packet { kind: u8; count: u32; }
        fn read_count(p: Packet) -> u32 { return p.count; }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn rejects_unknown_struct_member() {
    let output = check(
        r#"
        module test.member_missing;
        struct Point { x: i32; }
        fn bad(p: Point) -> i32 { return p.y; }
    "#,
    );
    assert!(
        has(&output, "type/unknown-field"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn struct_initializers_check_field_types_and_required_fields() {
    let wrong = check(
        r#"
        module test.struct_init_wrong;
        struct Packet { kind: u8; count: u32; }
        fn main() -> i32 {
            val p = Packet{kind: true, count: 1u32};
            return 0;
        }
    "#,
    );
    assert!(has(&wrong, "type/mismatch"), "{:?}", wrong.diagnostics);

    let missing = check(
        r#"
        module test.struct_init_missing;
        struct Packet { kind: u8; count: u32; }
        fn main() -> i32 {
            val p = Packet{kind: 1u8};
            return 0;
        }
    "#,
    );
    assert!(
        has(&missing, "type/missing-field"),
        "{:?}",
        missing.diagnostics
    );

    let defaulted = check(
        r#"
        module test.struct_init_default;
        struct Packet { kind: u8; count: u32 = 0u32; }
        fn main() -> i32 {
            val p = Packet{kind: 1u8};
            return 0;
        }
    "#,
    );
    assert!(
        defaulted.diagnostics.is_empty(),
        "{:?}",
        defaulted.diagnostics
    );
}

#[test]
fn mutable_reference_member_assignment_is_allowed() {
    let output = check(
        r#"
        module test.mutable_ref_member;
        struct Point { x: i32; }
        fn set_x(p: &mut Point) -> void {
            p.x = 7i32;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn immutable_reference_member_assignment_is_rejected() {
    let output = check(
        r#"
        module test.immutable_ref_member;
        struct Point { x: i32; }
        fn set_x(p: &Point) -> void {
            p.x = 7i32;
        }
    "#,
    );
    assert!(
        has(&output, "assignment/immutable"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn struct_pattern_binds_declared_field_types() {
    let output = check(
        r#"
        module test.struct_pattern_types;
        struct Pair { left: u8; right: u32; }
        fn right(pair: Pair) -> u32 {
            val Pair{left, right} = pair;
            val copy: u32 = right;
            return copy;
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|ty| *ty
        == Ty::Int {
            signed: false,
            width: IntWidth::W8
        }));
    assert!(body.local_types.values().any(|ty| *ty
        == Ty::Int {
            signed: false,
            width: IntWidth::W32
        }));
}

#[test]
fn rejects_struct_pattern_against_wrong_scrutinee_type() {
    let output = check(
        r#"
        module test.struct_pattern_mismatch;
        struct Point { x: i32; }
        struct Other { x: i32; }
        fn bad(value: Other) -> i32 {
            val Point{x} = value;
            return x;
        }
    "#,
    );
    assert!(has(&output, "pattern/type"), "{:?}", output.diagnostics);
}

#[test]
fn tagged_pattern_binds_payload_field_type() {
    let output = check(
        r#"
        module test.tagged_pattern_type;
        tagged Token { Number { value: i64; }, Plus, }
        fn read(token: Token) -> i64 {
            return match (token) {
                Token::Number{value} => value,
                Token::Plus => 0i64,
            };
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn rejects_unknown_pattern_field_and_variant() {
    let field = check(
        r#"
        module test.pattern_field_missing;
        struct Point { x: i32; }
        fn bad(point: Point) -> i32 {
            val Point{y} = point;
            return 0;
        }
    "#,
    );
    assert!(
        has(&field, "pattern/unknown-field"),
        "{:?}",
        field.diagnostics
    );

    let variant = check(
        r#"
        module test.pattern_variant_missing;
        tagged Token { Plus, }
        fn bad(token: Token) -> i32 {
            return match (token) { Token::Minus => 1, _ => 0 };
        }
    "#,
    );
    assert!(
        has(&variant, "pattern/unknown-variant"),
        "{:?}",
        variant.diagnostics
    );
}

#[test]
fn pattern_literals_and_option_patterns_are_checked_against_scrutinee() {
    let literal = check(
        r#"
        module test.pattern_literal_mismatch;
        fn bad(value: u32) -> i32 {
            return match (value) { true => 1, _ => 0 };
        }
    "#,
    );
    assert!(has(&literal, "pattern/type"), "{:?}", literal.diagnostics);

    let option = check(
        r#"
        module test.pattern_option_mismatch;
        fn bad(value: u32) -> i32 {
            return match (value) { Some(x) => i32(x), _ => 0 };
        }
    "#,
    );
    assert!(has(&option, "pattern/type"), "{:?}", option.diagnostics);
}

#[test]
fn or_pattern_bindings_must_have_identical_types() {
    let output = check(
        r#"
        module test.or_pattern_type;
        tagged Value {
            Count { x: u32; },
            Flag { x: bool; },
        }
        fn bad(value: Value) -> i32 {
            return match (value) {
                Value::Count{x} | Value::Flag{x} => 1,
                _ => 0,
            };
        }
    "#,
    );
    assert!(
        has(&output, "pattern/or-binding-type"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn or_pattern_same_binding_type_is_accepted() {
    let output = check(
        r#"
        module test.or_pattern_same_type;
        tagged Value {
            Left { x: u32; },
            Right { x: u32; },
        }
        fn read(value: Value) -> u32 {
            return match (value) {
                Value::Left{x} | Value::Right{x} => x,
            };
        }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn nested_struct_members_keep_exact_types() {
    let output = check(
        r#"
        module test.nested_member_type;
        struct Inner { value: u16; }
        struct Outer { inner: Inner; }
        fn read(outer: Outer) -> u16 { return outer.inner.value; }
    "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn direct_struct_member_assignment_tracks_root_mutability() {
    let mutable = check(
        r#"
        module test.mutable_struct_member;
        struct Point { x: i32; }
        fn main() -> i32 {
            var point = Point{x: 1i32};
            point.x = 2i32;
            return point.x;
        }
    "#,
    );
    assert!(mutable.diagnostics.is_empty(), "{:?}", mutable.diagnostics);

    let immutable = check(
        r#"
        module test.immutable_struct_member;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32};
            point.x = 2i32;
            return point.x;
        }
    "#,
    );
    assert!(
        has(&immutable, "assignment/immutable"),
        "{:?}",
        immutable.diagnostics
    );
}

#[test]
fn struct_and_tagged_constructors_reject_bad_field_sets() {
    let duplicate = check(
        r#"
        module test.duplicate_struct_field;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32, x: 2i32};
            return 0;
        }
    "#,
    );
    assert!(
        has(&duplicate, "type/duplicate-field"),
        "{:?}",
        duplicate.diagnostics
    );

    let unknown = check(
        r#"
        module test.unknown_struct_field;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32, y: 2i32};
            return 0;
        }
    "#,
    );
    assert!(
        has(&unknown, "type/unknown-field"),
        "{:?}",
        unknown.diagnostics
    );

    let tagged = check(
        r#"
        module test.tagged_constructor_fields;
        tagged Token { Number { value: i64; }, Plus, }
        fn main() -> i32 {
            val token = Token::Number{value: true};
            return 0;
        }
    "#,
    );
    assert!(has(&tagged, "type/mismatch"), "{:?}", tagged.diagnostics);
}

#[test]
fn optional_and_sequence_match_bindings_get_element_types() {
    let optional = check(
        r#"
        module test.optional_pattern_binding;
        fn read(value: u32?) -> u32 {
            return match (value) {
                Some(x) => x,
                None => 0u32,
            };
        }
    "#,
    );
    assert!(
        optional.diagnostics.is_empty(),
        "{:?}",
        optional.diagnostics
    );

    let sequence = check(
        r#"
        module test.sequence_pattern_binding;
        fn first(values: u16[]) -> u16 {
            return match (values) {
                [x, ..rest] => x,
                _ => 0u16,
            };
        }
    "#,
    );
    assert!(
        sequence.diagnostics.is_empty(),
        "{:?}",
        sequence.diagnostics
    );
}

#[test]
fn destructuring_declarations_reject_refutable_patterns() {
    let optional = check(
        r#"
        module test.refutable_optional_binding;
        fn read(value: u32?) -> u32 {
            val Some(x) = value;
            return x;
        }
    "#,
    );
    assert!(
        has(&optional, "pattern/refutable-binding"),
        "{:?}",
        optional.diagnostics
    );

    let slice = check(
        r#"
        module test.refutable_slice_binding;
        fn read(values: u32[]) -> u32 {
            val [first, ..rest] = values;
            return first;
        }
    "#,
    );
    assert!(
        has(&slice, "pattern/refutable-binding"),
        "{:?}",
        slice.diagnostics
    );
}
