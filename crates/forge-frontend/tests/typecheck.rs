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
fn value_for_requires_a_static_iterable_and_irrefutable_binding() {
    let scalar = check(
        r#"
        module test.value_for_scalar;
        fn main() {
            for (val value in 7u32) { value; }
        }
        "#,
    );
    assert!(
        has(&scalar, "type/for-each-iterable"),
        "{:?}",
        scalar.diagnostics
    );

    let refutable = check(
        r#"
        module test.value_for_refutable;
        fn main() {
            val values: [u32?; 1] = [Some(7u32)];
            for (val Some(value) in values) { value; }
        }
        "#,
    );
    assert!(
        has(&refutable, "pattern/refutable-binding"),
        "{:?}",
        refutable.diagnostics
    );
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
fn explicit_array_reference_slice_views_preserve_mutability() {
    let output = check(
        r#"
        module test.typecheck_slice_view;
        type ReadSlice = u32[];
        type WriteSlice = u32[] mut;
        fn clear(values: WriteSlice) { values[0] = 0u32; }
        fn main() -> i32 {
            var values: [u32; 2] = [1u32, 2u32];
            val read: ReadSlice = ReadSlice(&values);
            clear(WriteSlice(&mut values));
            val invalid: WriteSlice = WriteSlice(&values);
            return i32(read[0]);
        }
        "#,
    );
    assert!(has(&output, "type/mismatch"), "{:?}", output.diagnostics);
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
fn direct_global_member_assignment_tracks_root_mutability() {
    let mutable = check(
        r#"
        module test.mutable_global_member;
        struct Point { x: i32; }
        var POINT: Point = Point{x: 1i32};
        fn main() -> i32 {
            POINT.x = 2i32;
            return POINT.x;
        }
        "#,
    );
    assert!(mutable.diagnostics.is_empty(), "{:?}", mutable.diagnostics);

    let immutable = check(
        r#"
        module test.immutable_global_member;
        struct Point { x: i32; }
        val POINT: Point = Point{x: 1i32};
        fn main() -> i32 {
            POINT.x = 2i32;
            return POINT.x;
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
fn compiler_defined_sum_constructors_use_contextual_payload_types() {
    let output = check(
        r#"
        module test.sum_constructors;
        fn some() -> u32? { return Some(7u32); }
        fn ok() -> Result[u32, u8] { return Ok(9u32); }
        fn err() -> Result[u32, u8] { return Err(3u8); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);

    let missing_context = check(
        r#"
        module test.sum_constructor_context;
        fn bad() -> void { Ok(9u32); }
        "#,
    );
    assert!(
        has(&missing_context, "constructor/context"),
        "{:?}",
        missing_context.diagnostics
    );
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

#[test]
fn closure_plan_records_explicit_capture_modes_and_types() {
    let output = check(
        r#"
        module test.closure_capture_plan;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            var count: u32 = 0u32;
            val by_value = [factor](x: u32) -> u32 { return x * factor; };
            val by_ref = [&count]() -> u32 { return count; };
            val by_mut = [&mut count]() -> u32 {
                count = count + 1u32;
                return count;
            };
            by_value(2u32);
            by_ref();
            return by_mut();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().expect("main body");
    let plans = body
        .expressions
        .iter()
        .filter_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::ResolvedClosure { plan, .. } => Some(plan),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(plans.len(), 3);
    assert_eq!(
        plans[0].captures[0].mode,
        forge_frontend::CaptureMode::Value
    );
    assert_eq!(
        plans[1].captures[0].mode,
        forge_frontend::CaptureMode::SharedReference
    );
    assert_eq!(
        plans[2].captures[0].mode,
        forge_frontend::CaptureMode::MutableReference
    );
    assert_eq!(
        plans[0].captures[0].ty,
        forge_frontend::Ty::Int {
            signed: false,
            width: forge_frontend::IntWidth::W32,
        }
    );
}

#[test]
fn mutable_reference_closure_capture_requires_mutable_source() {
    let output = check(
        r#"
        module test.closure_bad_mut_capture;
        fn main() -> u32 {
            val count: u32 = 0u32;
            val next = [&mut count]() -> u32 { return count; };
            return next();
        }
        "#,
    );
    assert!(
        has(&output, "closure/mutable-capture"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn capture_free_closure_coerces_directly_to_function_pointer() {
    let output = check(
        r#"
        module test.closure_fn_pointer;
        fn main() -> u32 {
            val op: fn(u32) -> u32 = (x: u32) -> u32 { return x + 1u32; };
            return op(4u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(output
        .functions
        .values()
        .any(|body| body.expressions.iter().any(|expr| {
            matches!(
                &expr.kind,
                forge_frontend::TypedExprKind::ResolvedClosure { plan, .. }
                    if plan.function_pointer && plan.captures.is_empty()
            )
        })));
}

#[test]
fn closure_values_cannot_escape_by_return() {
    let output = check(
        r#"
        module test.closure_escape;
        fn bad(value: u32) -> closure(u32) -> u32 {
            return [value](x: u32) -> u32 { return x + value; };
        }
        "#,
    );
    assert!(has(&output, "closure/escape"), "{:?}", output.diagnostics);
}

#[test]
fn closure_values_cannot_cross_function_call_boundaries() {
    let source = r#"
        module test.closure_call_escape;
        fn apply(op: closure(u32) -> u32, value: u32) -> u32 {
            return op(value);
        }
        fn bad(value: u32) -> u32 {
            val add = [value](x: u32) -> u32 { return x + value; };
            return apply(add, 1u32);
        }
        "#;
    let output = check(source);
    let diagnostic = output
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "closure/escape")
        .expect("cross-function closure diagnostic");
    assert_eq!(diagnostic.span.start, source.find("add, 1u32").unwrap());
    assert!(diagnostic.message.contains("cannot cross a call boundary"));
    assert!(diagnostic.message.contains("Closure"));
    assert!(diagnostic.message.contains("capture-free function pointer"));
}

#[test]
fn execution_context_resolves_slots_and_scoped_types() {
    let output = check(
        r#"
        module test.context_slots;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            with context(:scratch = &scratch) {
                val current = context.scratch;
            }
            return scratch;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().expect("body");
    assert_eq!(body.context_scopes.len(), 1);
    assert!(matches!(
        body.context_scopes[0].overrides.as_slice(),
        [forge_frontend::TypedContextOverride {
            slot: forge_frontend::ContextSlot::Scratch,
            ty: Ty::Reference { mutable: false, .. },
            ..
        }]
    ));
    assert!(body.expressions.iter().any(|expr| matches!(
        (&expr.kind, &expr.ty),
        (
            forge_frontend::TypedExprKind::ResolvedContext {
                slot: forge_frontend::ContextSlot::Scratch,
                ..
            },
            Ty::Reference { mutable: false, inner }
        ) if inner.as_ref() == &Ty::Int { signed: false, width: forge_frontend::IntWidth::W32 }
    )));
}

#[test]
fn execution_context_rejects_unknown_duplicate_and_owning_overrides() {
    let output = check(
        r#"
        module test.context_errors;
        fn main() -> u32 {
            var a: u32 = 1u32;
            var b: u32 = 2u32;
            with context(:scratch = &a, :scratch = &b, :bogus = &a, :logger = a) {
                val current = context.scratch;
            }
            return a;
        }
        "#,
    );
    assert!(
        has(&output, "context/duplicate-override"),
        "{:?}",
        output.diagnostics
    );
    assert!(
        has(&output, "context/unknown-slot"),
        "{:?}",
        output.diagnostics
    );
    assert!(
        has(&output, "context/non-owning"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn execution_context_nested_override_types_are_lexically_scoped() {
    let output = check(
        r#"
        module test.context_nested;
        fn main() -> u32 {
            var outer: u32 = 1u32;
            var inner: u64 = 2u64;
            with context(:scratch = &outer) {
                val a = context.scratch;
                with context(:scratch = &inner) {
                    val b = context.scratch;
                }
                val c = context.scratch;
            }
            return outer;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().expect("body");
    let context_types = body
        .expressions
        .iter()
        .filter_map(|expr| {
            if matches!(
                expr.kind,
                forge_frontend::TypedExprKind::ResolvedContext { .. }
            ) {
                Some(expr.ty.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(context_types.len(), 3);
    assert!(matches!(&context_types[0], Ty::Reference { inner, .. }
        if inner.as_ref() == &Ty::Int { signed: false, width: forge_frontend::IntWidth::W32 }));
    assert!(matches!(&context_types[1], Ty::Reference { inner, .. }
        if inner.as_ref() == &Ty::Int { signed: false, width: forge_frontend::IntWidth::W64 }));
    assert!(matches!(&context_types[2], Ty::Reference { inner, .. }
        if inner.as_ref() == &Ty::Int { signed: false, width: forge_frontend::IntWidth::W32 }));
}

#[test]
fn select_resolves_channel_payload_timeout_and_runtime_operations() {
    let output = check(
        r#"
        module test.select_typed;
        struct Job { value: u32; }
        struct Jobs { marker: u8; }
        impl Jobs {
            fn recv(self: &Jobs) -> Job { return Job{value: 1u32}; }
        }
        fn worker(jobs: &Jobs) -> void {
            select {
                recv jobs -> Job{value} => { val copy: u32 = value; }
                timeout #duration "100ms" => { }
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output
        .functions
        .values()
        .find(|b| !b.select_plans.is_empty())
        .unwrap();
    let plan = &body.select_plans[0];
    assert_eq!(
        plan.operation,
        forge_frontend::RuntimeOperationId::SelectWait
    );
    assert!(matches!(
        &plan.arms[0],
        forge_frontend::TypedSelectArm::Receive {
            payload: Ty::Nominal(_),
            operation: forge_frontend::RuntimeOperationId::ChannelReceive,
            ..
        }
    ));
    assert!(matches!(
        &plan.arms[1],
        forge_frontend::TypedSelectArm::Timeout {
            operation: forge_frontend::RuntimeOperationId::SelectTimeout,
            ..
        }
    ));
}

#[test]
fn select_rejects_non_channel_and_wrong_timeout() {
    let bad_channel = check(
        r#"
        module test.select_bad_channel;
        fn worker(value: u32) -> void {
            select { recv value -> x => { } }
        }
        "#,
    );
    assert!(
        has(&bad_channel, "select/channel-type"),
        "{:?}",
        bad_channel.diagnostics
    );

    let bad_timeout = check(
        r#"
        module test.select_bad_timeout;
        fn worker() -> void {
            select { timeout 10u32 => { } }
        }
        "#,
    );
    assert!(
        has(&bad_timeout, "select/timeout-type"),
        "{:?}",
        bad_timeout.diagnostics
    );
}

#[test]
fn select_rejects_duplicate_timeout_arms() {
    let output = check(
        r#"
        module test.select_duplicate_timeout;
        fn worker() -> void {
            select {
                timeout #duration "1ms" => { }
                timeout #duration "2ms" => { }
            }
        }
        "#,
    );
    assert!(
        has(&output, "select/duplicate-timeout"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn raw_pointer_dereference_requires_unsafe_authorization() {
    let output = check(
        r#"
        module test.raw_deref_safe;
        fn read(p: *u32) -> u32 { return *p; }
        "#,
    );
    assert!(has(&output, "unsafe/required"), "{:?}", output.diagnostics);
}

#[test]
fn unsafe_raw_pointer_dereference_records_scope_provenance() {
    let output = check(
        r#"
        module test.raw_deref_unsafe;
        fn read(p: *u32) -> u32 {
            unsafe { return *p; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert_eq!(body.unsafe_scopes.len(), 1);
    let raw = body
        .expressions
        .iter()
        .find_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::UnsafeOperation {
                operation: forge_frontend::UnsafeOperationKind::RawDereference { volatile: false },
                provenance,
                ..
            } => Some((expr.span, *provenance)),
            _ => None,
        })
        .expect("typed raw dereference");
    assert_eq!(raw.1.scope, body.unsafe_scopes[0].span);
    assert!(raw.1.scope.start <= raw.0.start && raw.1.scope.end >= raw.0.end);
}

#[test]
fn safe_reference_dereference_needs_no_unsafe_provenance() {
    let output = check(
        r#"
        module test.reference_deref;
        fn read(p: &u32) -> u32 { return *p; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.unsafe_scopes.is_empty());
    assert!(!body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation { .. }
    )));
}

#[test]
fn raw_pointer_arithmetic_requires_and_records_unsafe() {
    let safe = check(
        r#"
        module test.pointer_offset_safe;
        fn next(p: *u32) -> *u32 { return p + 1usize; }
        "#,
    );
    assert!(has(&safe, "unsafe/required"), "{:?}", safe.diagnostics);

    let unsafe_output = check(
        r#"
        module test.pointer_offset_unsafe;
        fn next(p: *u32) -> *u32 {
            unsafe { return p + 1usize; }
        }
        "#,
    );
    assert!(
        unsafe_output.diagnostics.is_empty(),
        "{:?}",
        unsafe_output.diagnostics
    );
    let body = unsafe_output.functions.values().next().unwrap();
    assert!(body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation {
            operation: forge_frontend::UnsafeOperationKind::PointerOffset { subtract: false },
            ..
        }
    )));
}

#[test]
fn pointer_integer_and_reinterpret_conversions_require_unsafe() {
    let safe = check(
        r#"
        module test.pointer_cast_safe;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { return usize(p); }
        fn cast(p: *u32) -> BytePtr { return BytePtr(p); }
        "#,
    );
    assert!(
        safe.diagnostics
            .iter()
            .filter(|d| d.code == "unsafe/required")
            .count()
            >= 2,
        "{:?}",
        safe.diagnostics
    );

    let unsafe_output = check(
        r#"
        module test.pointer_cast_unsafe;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { unsafe { return usize(p); } }
        fn cast(p: *u32) -> BytePtr { unsafe { return BytePtr(p); } }
        "#,
    );
    assert!(
        unsafe_output.diagnostics.is_empty(),
        "{:?}",
        unsafe_output.diagnostics
    );
    let kinds = unsafe_output
        .functions
        .values()
        .flat_map(|body| body.expressions.iter())
        .filter_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::UnsafeOperation { operation, .. } => Some(*operation),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(kinds.contains(&forge_frontend::UnsafeOperationKind::PointerToInteger));
    assert!(kinds.contains(&forge_frontend::UnsafeOperationKind::PointerReinterpret));
}

#[test]
fn integer_to_pointer_alias_conversion_requires_unsafe() {
    let output = check(
        r#"
        module test.address_to_pointer;
        type Raw = *u32;
        fn from_address(address: usize) -> Raw {
            unsafe { return Raw(address); }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation {
            operation: forge_frontend::UnsafeOperationKind::IntegerToPointer,
            ..
        }
    )));
}

#[test]
fn bitstruct_layout_is_lsb_first_with_standard_field_types() {
    let output = check(
        r#"
        module test.bitstruct_layout;
        bitstruct Status: u16 {
            ready: 1;
            error: 1;
            mode: 3;
            code: 5;
            reserved: 6;
        }
        fn main(status: Status) -> u8 { return status.mode; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let layout = output.bitstructs.values().next().expect("bitstruct layout");
    assert_eq!(layout.storage_bits, 16);
    assert_eq!(layout.fields["ready"].offset, 0);
    assert_eq!(layout.fields["error"].offset, 1);
    assert_eq!(layout.fields["mode"].offset, 2);
    assert_eq!(layout.fields["code"].offset, 5);
    assert_eq!(layout.fields["reserved"].offset, 10);
    assert_eq!(layout.fields["ready"].ty, Ty::Bool);
    assert_eq!(
        layout.fields["mode"].ty,
        Ty::Int {
            signed: false,
            width: IntWidth::W8,
        }
    );
    assert!(output
        .functions
        .values()
        .flat_map(|body| body.expressions.iter())
        .any(|expr| matches!(
            expr.kind,
            forge_frontend::TypedExprKind::ResolvedBitField { .. }
        )));
}

#[test]
fn bitstruct_rejects_invalid_storage_and_overflowing_layout() {
    let bad_storage = check(
        r#"
        module test.bitstruct_bad_storage;
        bitstruct Bad: i16 { x: 1; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(
        has(&bad_storage, "bitstruct/storage"),
        "{:?}",
        bad_storage.diagnostics
    );

    let too_wide = check(
        r#"
        module test.bitstruct_too_wide;
        bitstruct Bad: u8 { a: 5; b: 4; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(
        has(&too_wide, "bitstruct/width"),
        "{:?}",
        too_wide.diagnostics
    );
}

#[test]
fn bitstruct_constructor_requires_exact_storage_type() {
    let output = check(
        r#"
        module test.bitstruct_constructor;
        bitstruct Status: u16 { mode: 3; }
        fn main() -> i32 {
            val status: Status = Status(0u8);
            return 0;
        }
        "#,
    );
    assert!(
        has(&output, "bitstruct/storage-conversion"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn map_pattern_protocol_types_required_and_optional_bindings() {
    let output = check(
        r#"
        module test.map_protocol;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name, :age age?, ..} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output
        .functions
        .values()
        .find(|body| {
            body.return_type
                == Ty::Int {
                    signed: false,
                    width: IntWidth::W32,
                }
        })
        .unwrap();
    assert!(body.local_types.values().any(|ty| *ty
        == Ty::Optional {
            inner: Box::new(Ty::Int {
                signed: false,
                width: IntWidth::W32
            })
        }));
}

#[test]
fn map_pattern_requires_complete_collection_protocol() {
    let output = check(
        r#"
        module test.map_protocol_missing;
        struct Dict {}
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name, ..} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(
        has(&output, "pattern/collection-protocol"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn map_pattern_rejects_duplicate_keys() {
    let output = check(
        r#"
        module test.map_duplicate;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name first, :name second, ..} => first,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(
        has(&output, "pattern/duplicate-key"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn runtime_global_initializers_are_dependency_ordered_and_constants_are_static() {
    let output = check(
        r#"
        module test.global_init_order;
        fn seed() -> u32 { return 3u32; }
        val dependent: u32 = base + 1u32;
        val base: u32 = seed();
        const fixed: u32 = 7u32;
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.global_initializers.len(), 2);
    assert_eq!(output.global_init_order.len(), 2);
    assert_eq!(output.constants.len(), 1);

    let first = output.global_init_order[0];
    let second = output.global_init_order[1];
    assert!(output.global_initializers[&first].dependencies.is_empty());
    assert_eq!(
        output.global_initializers[&second].dependencies,
        vec![first]
    );
    assert!(
        first.0 > second.0,
        "dependency declared later must initialize first"
    );
}

#[test]
fn runtime_global_initializer_cycles_are_rejected() {
    let output = check(
        r#"
        module test.global_init_cycle;
        val first: u32 = second + 1u32;
        val second: u32 = first + 1u32;
        "#,
    );
    assert!(
        has(&output, "global/init-cycle"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn independent_runtime_globals_keep_source_order() {
    let output = check(
        r#"
        module test.global_init_source_order;
        fn seed() -> u32 { return 1u32; }
        val first: u32 = seed();
        val second: u32 = seed();
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let sorted = output
        .global_initializers
        .keys()
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(output.global_init_order, sorted);
}

#[test]
fn result_constructors_and_patterns_require_a_compatible_result_context() {
    let nested = check(
        r#"
        module test.result_nested;
        fn nested() -> Result[Result[u8, u16], u32] {
            return Ok(Err(7u16));
        }
        fn inspect(value: Result[u8, u16]) -> u8 {
            return match (value) { Ok(payload) => payload, Err(_) => 0u8, };
        }
        "#,
    );
    assert!(nested.diagnostics.is_empty(), "{:?}", nested.diagnostics);

    let invalid = check(
        r#"
        module test.result_invalid;
        fn bad_constructor() -> void { Ok(1u8); }
        fn bad_pattern(value: u8) -> u8 {
            return match (value) { Ok(payload) => payload, _ => 0u8, };
        }
        fn bad_propagation(value: Result[u8, u16]) -> Result[u8, u8] { return value?; }
        "#,
    );
    assert!(
        has(&invalid, "constructor/context"),
        "{:?}",
        invalid.diagnostics
    );
    assert!(
        has(&invalid, "pattern/result-ok"),
        "{:?}",
        invalid.diagnostics
    );
    assert!(has(&invalid, "try/error-type"), "{:?}", invalid.diagnostics);

    let invalid_payloadless = check(
        r#"
        module test.result_invalid_payloadless;
        fn bad() -> Result[u8, u8] { return Ok(); }
        "#,
    );
    assert!(
        has(&invalid_payloadless, "constructor/arguments"),
        "{:?}",
        invalid_payloadless.diagnostics
    );
}

#[test]
fn nested_payloadless_results_match_and_propagate_with_precise_errors() {
    let valid = check(
        r#"
        module test.result_nested_payloadless;
        fn source(ok: bool) -> Result[void, void] {
            if (ok) { return Ok(); }
            return Err();
        }
        fn nested(ok: bool) -> Result[Result[void, u8], void] {
            source(ok)?;
            return Ok(Ok());
        }
        fn inspect(value: Result[Result[void, u8], void]) -> u8 {
            return match (value) {
                Ok(Ok(_)) => 1u8,
                Ok(Err(_)) => 2u8,
                Ok(_) => 4u8,
                Err(_) => 3u8,
            };
        }
        "#,
    );
    assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);

    let invalid = check(
        r#"
        module test.result_precise_errors;
        fn source() -> Result[u8, u16] { return Err(1u16); }
        fn bad_try() -> Result[u8, u8] { return source()?; }
        fn bad_arms(value: bool) -> Result[u8, u8] {
            return match (value) { true => Ok(1u8), false => Err(1u16), };
        }
        "#,
    );
    let propagation = invalid
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "try/error-type")
        .expect("incompatible propagation diagnostic");
    assert!(propagation.span.start < propagation.span.end);
    assert!(propagation.message.contains("cannot propagate error type"));
    let arm = invalid
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "constructor/payload-type")
        .expect("incompatible Result arm diagnostic");
    assert!(arm.span.start < arm.span.end);
}
