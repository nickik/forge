use forge_frontend::{
    lower_fir, lower_module, lower_resolved_bodies, parse_source, type_check_module,
    FirInstructionKind, FirTerminator, OverflowMode, Ty, TypedExprKind,
};

fn lower(source: &str) -> forge_frontend::FirOutput {
    let parsed = parse_source(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("AST");
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "hir: {:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(
        bodies.diagnostics.is_empty(),
        "body hir: {:?}",
        bodies.diagnostics
    );
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(
        typed.diagnostics.is_empty(),
        "typed: {:?}",
        typed.diagnostics
    );
    lower_fir(&bodies, &typed)
}

fn instructions(output: &forge_frontend::FirOutput) -> impl Iterator<Item = &FirInstructionKind> {
    output
        .module
        .functions
        .values()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .map(|i| &i.kind)
}

#[test]
fn lowers_checked_arithmetic_and_cfg() {
    let output = lower(
        r#"
        module test.fir_arithmetic;
        fn add(a: u32, b: u32) -> u32 {
            var x: u32 = a + b;
            if (x > 10u32) { x = x - 1u32; }
            while (x < 20u32) { x = x + 1u32; }
            return x;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Add,
            overflow: Some(OverflowMode::Checked),
            ..
        }
    )));
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.len() >= 6);
    assert!(function.blocks.iter().all(|b| b.terminator.is_some()));
}

#[test]
fn overflow_metadata_selects_wrapping_fir_operation() {
    let output = lower(
        r#"
        module test.fir_wrap;
        @overflow(wrap)
        fn add(a: u32, b: u32) -> u32 { return a + b; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Add,
            overflow: Some(OverflowMode::Wrapping),
            ..
        }
    )));
}

#[test]
fn indexing_has_explicit_bounds_check() {
    let output = lower(
        r#"
        module test.fir_bounds;
        fn read(values: [u32; 4], index: usize) -> u32 { return values[index]; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::BoundsCheck { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::IndexUnchecked { .. })));
}

#[test]
fn result_try_is_explicit_cfg_with_error_return() {
    let output = lower(
        r#"
        module test.fir_try;
        fn pass(value: Result[u32, u8]) -> Result[u32, u8] {
            value?;
            return value;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultIsOk { .. })));
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultUnwrapErr { .. }))
    );
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeResultErr { .. })));
    let function = output.module.functions.values().next().unwrap();
    assert!(
        function
            .blocks
            .iter()
            .filter(|b| matches!(b.terminator, Some(FirTerminator::Return { .. })))
            .count()
            >= 2
    );
}

#[test]
fn optional_promotion_is_retained_and_lowered_to_some() {
    let parsed = parse_source(
        r#"
        module test.fir_optional;
        fn maybe(value: u32) -> u32? { return value; }
        "#,
    );
    assert!(parsed.diagnostics.is_empty());
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    assert!(typed.functions.values().any(|body| body
        .expressions
        .iter()
        .any(|expr| matches!(expr.kind, TypedExprKind::OptionalPromote { .. }))));
    let output = lower_fir(&bodies, &typed);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeSome { .. })));
}

#[test]
fn method_reference_receiver_becomes_explicit_address() {
    let output = lower(
        r#"
        module test.fir_method;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
        }
        fn main() -> i32 {
            val point = Point{x: 7i32};
            return point.get();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::AddressOf { mutable: false, .. })));
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::Call { args, .. } if !args.is_empty())));
}

#[test]
fn defer_call_is_emitted_before_return() {
    let output = lower(
        r#"
        module test.fir_defer;
        fn cleanup() -> void { return; }
        fn main() -> i32 {
            defer cleanup();
            return 7i32;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output
        .module
        .functions
        .values()
        .find(|f| {
            f.return_type
                == Ty::Int {
                    signed: true,
                    width: forge_frontend::IntWidth::W32,
                }
        })
        .unwrap();
    let return_block = main
        .blocks
        .iter()
        .find(|b| matches!(b.terminator, Some(FirTerminator::Return { .. })))
        .unwrap();
    assert!(return_block
        .instructions
        .iter()
        .any(|i| matches!(i.kind, FirInstructionKind::Call { .. })));
}

#[test]
fn boolean_match_plan_lowers_to_cfg() {
    let output = lower(
        r#"
        module test.fir_bool_match;
        fn choose(value: bool) -> i32 {
            return match (value) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.len() >= 5);
    assert!(function
        .blocks
        .iter()
        .all(|block| block.terminator.is_some()));
    assert!(function
        .blocks
        .iter()
        .any(|block| matches!(block.terminator, Some(FirTerminator::Branch { .. }))));
}

#[test]
fn boolean_match_preserves_wildcard_and_guard_fallthrough() {
    let output = lower(
        r#"
        module test.fir_bool_guard;
        fn choose(value: bool, guard: bool) -> i32 {
            return match (value) {
                true when guard => 1i32,
                true => 2i32,
                _ => 3i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let branches = output
        .module
        .functions
        .values()
        .flat_map(|function| &function.blocks)
        .filter(|block| matches!(block.terminator, Some(FirTerminator::Branch { .. })))
        .count();
    assert!(branches >= 3, "expected pattern and guard branches");
}

#[test]
fn boolean_match_scrutinee_is_evaluated_once() {
    let output = lower(
        r#"
        module test.fir_bool_once;
        fn source() -> bool { return true; }
        fn choose() -> i32 {
            return match (source()) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let calls = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::Call { .. }))
        .count();
    assert_eq!(calls, 1, "match scrutinee call must execute once");
}

#[test]
fn boolean_match_block_arms_lower_as_void_cfg() {
    let output = lower(
        r#"
        module test.fir_bool_blocks;
        fn choose(value: bool) -> void {
            match (value) {
                true => { val x: u32 = 1u32; },
                false => { val y: u32 = 2u32; },
            };
            return;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Unit)));
}

#[test]
fn optional_match_extracts_payload_before_guard_and_body() {
    let output = lower(
        r#"
        module test.fir_option_match;
        fn choose(value: u32?) -> u32 {
            return match (value) {
                Some(x) when x > 10u32 => x,
                Some(x) => x,
                None => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionIsSome { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionUnwrap { .. })));
}

#[test]
fn enum_match_uses_resolved_variant_tests() {
    let output = lower(
        r#"
        module test.fir_enum_match;
        enum Color { Red, Green, Blue }
        fn choose(value: Color) -> i32 {
            return match (value) {
                Color::Red => 1i32,
                Color::Green => 2i32,
                Color::Blue => 3i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let tests = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::VariantIs { .. }))
        .count();
    assert_eq!(tests, 3);
}

#[test]
fn tagged_match_extracts_typed_payload_bindings() {
    let output = lower(
        r#"
        module test.fir_tagged_match;
        tagged Token {
            Number { value: u32; },
            Empty,
        }
        fn read(token: Token, gate: bool) -> u32 {
            return match (token) {
                Token::Number{value} when gate => value,
                Token::Number{value} => value,
                Token::Empty => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::VariantIs { .. })));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "value"
    )));
}

#[test]
fn map_match_still_waits_for_collection_pattern_protocol() {
    let output = lower(
        r#"
        module test.fir_map_match_later;
        fn choose(values: u32[]) -> i32 {
            return match (values) { {:name ignored, ..} => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}

#[test]
fn scalar_integer_literal_match_lowers_to_equality() {
    let output = lower(
        r#"
        module test.fir_scalar_integer;
        fn choose(value: i32) -> i32 {
            return match (value) { 7 => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Eq,
            ..
        }
    )));
}

#[test]
fn scalar_char_range_match_lowers_both_bounds() {
    let output = lower(
        r#"
        module test.fir_scalar_range;
        fn choose(value: char) -> i32 {
            return match (value) { 'a'..='z' => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::GreaterEq,
            ..
        }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::LessEq,
            ..
        }
    )));
}

#[test]
fn scalar_string_literal_match_is_resolved_before_fir() {
    let output = lower(
        r#"
        module test.fir_scalar_string;
        fn choose(value: str) -> i32 {
            return match (value) { "yes" => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}

#[test]
fn struct_pattern_lowers_nested_field_test_and_binding() {
    let output = lower(
        r#"
        module test.fir_struct_pattern;
        struct Point { x: i32; y: i32; }
        fn choose(point: Point) -> i32 {
            return match (point) {
                Point{x: 7, y} => y,
                _ => 0i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "x"
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "y"
    )));
}

#[test]
fn sequence_rest_pattern_lowers_length_index_and_tail() {
    let output = lower(
        r#"
        module test.fir_sequence_pattern;
        fn choose(values: u32[]) -> u32 {
            return match (values) {
                [first, second, ..rest] => first + second,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Len { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::IndexUnchecked { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Subsequence { .. })));
}

#[test]
fn or_pattern_uses_separate_resolved_alternatives() {
    let output = lower(
        r#"
        module test.fir_or_pattern;
        enum Token { Plus, Minus, Number }
        fn choose(token: Token) -> i32 {
            return match (token) {
                Token::Plus | Token::Minus => 1i32,
                _ => 0i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let variants = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::VariantIs { .. }))
        .count();
    assert_eq!(variants, 2);
}

#[test]
fn as_pattern_binds_whole_value_before_body() {
    let output = lower(
        r#"
        module test.fir_as_pattern;
        struct Point { x: i32; y: i32; }
        fn choose(point: Point) -> i32 {
            return match (point) {
                whole @ Point{x, y} => whole.x + x + y,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Store { .. }))
            .count()
            >= 3
    );
}
