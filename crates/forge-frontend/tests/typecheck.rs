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

#[test]
fn typed_hir_preserves_generic_metadata_table() {
    let output = check(
        r#"
        module test.metadata_typed;
        @inline
        @overflow(wrap)
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let metadata = output
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(0) })
        .expect("typed metadata");
    assert!(metadata.iter().any(|m| m.name.as_deref() == Some("inline")));
    assert!(metadata
        .iter()
        .any(|m| m.name.as_deref() == Some("overflow")));
}

#[test]
fn fixed_array_length_is_preserved_and_used_for_irrefutable_patterns() {
    let output = check(
        r#"
        module test.array_length;
        fn head(pair: [u32; 2]) -> u32 {
            val [left, right] = pair;
            return left + right;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array {
            length: Some(2),
            ..
        }
    )));
}

#[test]
fn declaration_defaults_are_checked_early() {
    let output = check(
        r#"
        module test.bad_default;
        struct Config { retries: u8 = true; }
        nfn connect(port: u16 = false) -> bool { return true; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "type/declaration-default")
            .count()
            >= 2,
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn method_calls_resolve_to_method_defids() {
    let output = check(
        r#"
        module test.method_defid;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
            fn set(self: &mut Point, x: i32) -> void { self.x = x; }
        }
        fn main() -> i32 {
            var p = Point{x: 1};
            p.set(2);
            return p.get();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let mut method_targets = output
        .functions
        .values()
        .flat_map(|body| body.expressions.iter())
        .filter_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::ResolvedCall {
                target,
                method: true,
                ..
            } => Some(*target),
            _ => None,
        })
        .collect::<Vec<_>>();
    method_targets.sort();
    method_targets.dedup();
    assert_eq!(method_targets.len(), 2, "{method_targets:?}");
}

#[test]
fn mutable_method_rejects_immutable_receiver() {
    let output = check(
        r#"
        module test.method_mutability;
        struct Point { x: i32; }
        impl Point {
            fn set(self: &mut Point, x: i32) -> void { self.x = x; }
        }
        fn main() -> i32 {
            val p = Point{x: 1};
            p.set(2);
            return 0;
        }
        "#,
    );
    assert!(
        has(&output, "method/immutable-receiver"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn finite_matches_are_checked_for_exhaustiveness() {
    let output = check(
        r#"
        module test.exhaustive;
        enum Color { Red, Green }
        fn bad(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
            };
        }
        "#,
    );
    assert!(
        has(&output, "match/non-exhaustive"),
        "{:?}",
        output.diagnostics
    );

    let ok = check(
        r#"
        module test.exhaustive_ok;
        enum Color { Red, Green }
        fn good(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
                Color::Green => 2,
            };
        }
        "#,
    );
    assert!(ok.diagnostics.is_empty(), "{:?}", ok.diagnostics);
}

#[test]
fn finite_matches_report_provably_unreachable_arms() {
    let duplicate = check(
        r#"
        module test.unreachable_enum_arm;
        enum Color { Red, Green }
        fn code(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
                Color::Red => 2,
                Color::Green => 3,
            };
        }
        "#,
    );
    assert!(
        has(&duplicate, "match/unreachable-arm"),
        "{:?}",
        duplicate.diagnostics
    );

    let after_wildcard = check(
        r#"
        module test.unreachable_after_wildcard;
        enum Color { Red, Green }
        fn code(color: Color) -> i32 {
            return match (color) {
                _ => 1,
                Color::Green => 2,
            };
        }
        "#,
    );
    assert!(
        has(&after_wildcard, "match/unreachable-arm"),
        "{:?}",
        after_wildcard.diagnostics
    );

    let guarded = check(
        r#"
        module test.guarded_arm_does_not_cover;
        enum Color { Red, Green }
        fn choose(color: Color, flag: bool) -> i32 {
            return match (color) {
                Color::Red when flag => 1,
                Color::Red => 2,
                Color::Green => 3,
            };
        }
        "#,
    );
    assert!(guarded.diagnostics.is_empty(), "{:?}", guarded.diagnostics);
}

#[test]
fn result_try_is_resolved_and_requires_compatible_enclosing_result() {
    let valid = check(
        r#"
        module test.try_valid;
        fn source() -> Result[u32, u8] { return source(); }
        fn propagate() -> Result[u32, u8] {
            val value = source()?;
            return source();
        }
        "#,
    );
    assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);
    let body = valid.functions.get(&DefId(1)).expect("propagate body");
    assert!(body.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedTry {
            source_error: Ty::Int {
                signed: false,
                width: IntWidth::W8
            },
            target_error: Ty::Int {
                signed: false,
                width: IntWidth::W8
            },
            ..
        }
    )));

    let mismatch = check(
        r#"
        module test.try_mismatch;
        fn source() -> Result[u32, u8] { return source(); }
        fn target() -> Result[u32, u16] {
            val value = source()?;
            return target();
        }
        "#,
    );
    assert!(
        has(&mismatch, "try/error-type"),
        "{:?}",
        mismatch.diagnostics
    );

    let plain = check(
        r#"
        module test.try_plain;
        fn source() -> Result[u32, u8] { return source(); }
        fn target() -> u32 {
            val value = source()?;
            return value;
        }
        "#,
    );
    assert!(has(&plain, "try/context"), "{:?}", plain.diagnostics);

    let optional = check(
        r#"
        module test.try_optional;
        fn target(value: u32?) -> u32 {
            return value?;
        }
        "#,
    );
    assert!(has(&optional, "try/operand"), "{:?}", optional.diagnostics);
}

#[test]
fn constants_feed_array_lengths_and_enum_values() {
    let output = check(
        r#"
        module test.constants;
        const WIDTH: usize = 2;
        const COUNT: usize = WIDTH * 2;
        enum Code {
            Small = WIDTH,
            Large = COUNT + 1,
        }
        fn total(values: [u32; COUNT]) -> u32 {
            val [a, b, c, d] = values;
            return a + b + c + d;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        output.constants.get(&DefId(0)),
        Some(&forge_frontend::ConstValue::Integer { value: 2 })
    );
    assert_eq!(
        output.constants.get(&DefId(1)),
        Some(&forge_frontend::ConstValue::Integer { value: 4 })
    );
    assert_eq!(
        output
            .enum_values
            .get(&DefId(2))
            .and_then(|values| values.get("Small")),
        Some(&2)
    );
    assert_eq!(
        output
            .enum_values
            .get(&DefId(2))
            .and_then(|values| values.get("Large")),
        Some(&5)
    );
    let body = output.functions.get(&DefId(3)).expect("total body");
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array {
            length: Some(4),
            ..
        }
    )));
}

#[test]
fn constant_evaluation_rejects_cycles_and_runtime_calls() {
    let cycle = check(
        r#"
        module test.const_cycle;
        const A: usize = B + 1;
        const B: usize = A + 1;
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&cycle, "const/eval"), "{:?}", cycle.diagnostics);
    assert!(cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("A -> B -> A")));

    let runtime = check(
        r#"
        module test.const_runtime;
        fn runtime() -> usize { return 1usize; }
        const BAD: usize = runtime();
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&runtime, "const/eval"), "{:?}", runtime.diagnostics);
}

#[test]
fn array_length_rejects_non_const_global() {
    let output = check(
        r#"
        module test.array_non_const;
        val COUNT: usize = 4usize;
        fn total(values: [u32; COUNT]) -> u32 { return 0u32; }
        "#,
    );
    assert!(
        has(&output, "type/array-length"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn local_consts_are_checked_retained_and_usable_in_array_lengths() {
    let output = check(
        r#"
        module test.local_const;
        fn main() -> u32 {
            const WIDTH: usize = 2;
            const COUNT: usize = WIDTH * 2;
            val values: [u32; COUNT] = [1u32, 2u32, 3u32, 4u32];
            val [a, b, c, d] = values;
            return a + b + c + d;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.get(&DefId(0)).expect("main body");
    assert!(body
        .local_constants
        .values()
        .any(|value| *value == forge_frontend::ConstValue::Integer { value: 2 }));
    assert!(body
        .local_constants
        .values()
        .any(|value| *value == forge_frontend::ConstValue::Integer { value: 4 }));
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array {
            length: Some(4),
            ..
        }
    )));

    let runtime = check(
        r#"
        module test.local_const_runtime;
        fn runtime() -> usize { return 4usize; }
        fn main() -> i32 {
            const BAD: usize = runtime();
            return 0;
        }
        "#,
    );
    assert!(has(&runtime, "const/eval"), "{:?}", runtime.diagnostics);
}

#[test]
fn typed_hir_retains_fir_boundary_facts() {
    let output = check(
        r#"
        module test.fir_boundary_facts;
        struct Point { x: u32; }
        impl Point { fn get(self: &Point) -> u32 { return self.x; } }
        nfn combine(left: u32, right: u32) -> u32 { return left + right; }
        fn maybe(point: Point) -> u32? {
            val x = combine(:right = 2u32, :left = point.get());
            return x;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let maybe = output
        .functions
        .values()
        .find(|body| matches!(body.return_type, Ty::Optional { .. }))
        .expect("maybe body");
    assert_eq!(maybe.params.len(), 1);
    assert!(maybe.expressions.iter().all(|expr| expr.id.0 < u32::MAX));
    assert!(maybe.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedCall { arguments, .. }
            if matches!(
                arguments.as_slice(),
                [
                    forge_frontend::ResolvedCallArgument::Explicit { argument: 1 },
                    forge_frontend::ResolvedCallArgument::Explicit { argument: 0 },
                ]
            )
    )));
    assert!(maybe.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::OptionalPromote { .. }
    )));
}

#[test]
fn match_plan_nested_payload_test_does_not_cover_entire_variant() {
    let output = check(
        r#"
        module test.match_plan_nested_non_exhaustive;
        tagged Token { Number { value: u32; }, Plus, }
        fn code(token: Token) -> i32 {
            return match (token) {
                Token::Number{value: 1} => 1,
                Token::Plus => 2,
            };
        }
        "#,
    );
    assert!(
        has(&output, "match/non-exhaustive"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn match_plan_some_binding_subsumes_later_payload_literal() {
    let output = check(
        r#"
        module test.match_plan_optional_subsumption;
        fn code(value: u32?) -> i32 {
            return match (value) {
                Some(_) => 1,
                Some(7) => 2,
                None => 3,
            };
        }
        "#,
    );
    assert!(
        has(&output, "match/unreachable-arm"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn match_plan_variant_binding_subsumes_later_field_literal() {
    let output = check(
        r#"
        module test.match_plan_variant_subsumption;
        tagged Value { Count { x: u32; }, Flag, }
        fn code(value: Value) -> i32 {
            return match (value) {
                Value::Count{x} => 1,
                Value::Count{x: 7} => 2,
                Value::Flag => 3,
            };
        }
        "#,
    );
    assert!(
        has(&output, "match/unreachable-arm"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn match_plan_or_alternatives_cover_closed_domain() {
    let output = check(
        r#"
        module test.match_plan_or_exhaustive;
        enum Token { Plus, Minus, Number }
        fn code(token: Token) -> i32 {
            return match (token) {
                Token::Plus | Token::Minus => 1,
                Token::Number => 2,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn named_call_plan_materializes_defaults_in_final_parameter_order() {
    let output = check(
        r#"
        module test.normalized_defaults;
        nfn combine(first: u32, second: u32 = first + 1u32, third: u32 = second + 1u32) -> u32 {
            return first + second + third;
        }
        fn main() -> u32 {
            return combine(:third = 9u32, :first = 3u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let arguments = output
        .functions
        .values()
        .flat_map(|body| &body.expressions)
        .find_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::ResolvedCall { arguments, .. }
                if arguments.len() == 3 =>
            {
                Some(arguments)
            }
            _ => None,
        })
        .expect("normalized three-argument call plan");
    assert!(matches!(
        arguments.as_slice(),
        [
            forge_frontend::ResolvedCallArgument::Explicit { argument: 1 },
            forge_frontend::ResolvedCallArgument::Default { .. },
            forge_frontend::ResolvedCallArgument::Explicit { argument: 0 },
        ]
    ));
}
