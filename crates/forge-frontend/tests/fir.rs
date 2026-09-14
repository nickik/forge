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

#[test]
fn normalized_defaults_lower_without_fir_default_reconstruction() {
    let output = lower(
        r#"
        module test.fir_defaults;
        fn source() -> u32 { return 3u32; }
        nfn combine(first: u32, second: u32 = first + 1u32, third: u32 = second + 1u32) -> u32 {
            return first + second + third;
        }
        fn main() -> u32 {
            return combine(:third = 9u32, :first = source());
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!instructions(&output).any(|op| matches!(op, FirInstructionKind::Poison)));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Call { args, .. } if args.len() == 3
    )));
    assert_eq!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Call { args, .. } if args.is_empty()))
            .count(),
        1,
        "explicit source() argument must be evaluated exactly once"
    );
}

#[test]
fn chained_defaults_can_read_earlier_materialized_parameters() {
    let output = lower(
        r#"
        module test.fir_chained_defaults;
        nfn advance(first: u32, second: u32 = first + 1u32, third: u32 = second + 1u32) -> u32 {
            return third;
        }
        fn main() -> u32 { return advance(:first = 5u32); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Call { args, .. } if args.len() == 3
    )));
    assert!(
        instructions(&output)
            .filter(|op| matches!(
                op,
                FirInstructionKind::Binary {
                    op: forge_frontend::ast::BinaryOp::Add,
                    ..
                }
            ))
            .count()
            >= 2
    );
}

#[test]
fn captured_closure_lowers_environment_body_and_call() {
    let output = lower(
        r#"
        module test.fir_closure_capture;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            val scale = [factor](x: u32) -> u32 { return x * factor; };
            return scale(3u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    assert_eq!(main.closures.len(), 1);
    let closure = main.closures.values().next().unwrap();
    assert_eq!(closure.captures.len(), 1);
    assert_eq!(closure.captures[0].mode, forge_frontend::CaptureMode::Value);
    assert!(main
        .blocks
        .iter()
        .any(|block| block.closure == Some(closure.id)));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::MakeClosure { captures, .. } if captures.len() == 1
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::CallClosure { .. })));
}

#[test]
fn mutable_reference_closure_uses_aliasing_capture_place() {
    let output = lower(
        r#"
        module test.fir_closure_mut_ref;
        fn main() -> u32 {
            var count: u32 = 0u32;
            val next = [&mut count]() -> u32 {
                count = count + 1u32;
                return count;
            };
            next();
            return count;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    let closure = main.closures.values().next().unwrap();
    assert_eq!(
        closure.captures[0].mode,
        forge_frontend::CaptureMode::MutableReference
    );
    assert!(main
        .blocks
        .iter()
        .filter(|block| block.closure == Some(closure.id))
        .any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.kind,
                    FirInstructionKind::Store {
                        place: forge_frontend::FirPlace::ClosureCapture { .. },
                        ..
                    }
                )
            })
        }));
}

#[test]
fn capture_free_closure_function_pointer_has_no_environment() {
    let output = lower(
        r#"
        module test.fir_closure_fn_ptr;
        fn main() -> u32 {
            val op: fn(u32) -> u32 = (x: u32) -> u32 { return x + 1u32; };
            return op(8u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    let closure = main.closures.values().next().unwrap();
    assert!(closure.function_pointer);
    assert!(closure.captures.is_empty());
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::MakeClosure { captures, .. } if captures.is_empty()
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::CallIndirect { .. })));
}

#[test]
fn execution_context_lowers_save_set_load_and_restore() {
    let output = lower(
        r#"
        module test.context_fir;
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
    let ops = instructions(&output).collect::<Vec<_>>();
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextSave {
            slot: forge_frontend::ContextSlot::Scratch
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextSet {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextLoad {
            slot: forge_frontend::ContextSlot::Scratch
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextRestore {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
    assert!(!output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/context-not-resolved"));
}

#[test]
fn execution_context_restore_runs_on_return_cleanup_edge() {
    let output = lower(
        r#"
        module test.context_return;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            with context(:scratch = &scratch) {
                return scratch;
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().unwrap();
    let return_block = function
        .blocks
        .iter()
        .find(|block| matches!(block.terminator, Some(FirTerminator::Return { .. })))
        .expect("return block");
    assert!(return_block.instructions.iter().any(|instruction| matches!(
        instruction.kind,
        FirInstructionKind::ContextRestore {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
}

#[test]
fn nested_execution_context_restores_in_lifo_order() {
    let output = lower(
        r#"
        module test.context_nested_fir;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            var logger: u64 = 9u64;
            with context(:scratch = &scratch, :logger = &logger) {
                val a = context.scratch;
                val b = context.logger;
            }
            return scratch;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let restores = instructions(&output)
        .filter_map(|op| match op {
            FirInstructionKind::ContextRestore { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        restores,
        vec![
            forge_frontend::ContextSlot::Logger,
            forge_frontend::ContextSlot::Scratch,
        ]
    );
}

#[test]
fn select_lowers_to_runtime_select_cfg_with_payload_binding() {
    let output = lower(
        r#"
        module test.fir_select;
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
    let worker = output
        .module
        .functions
        .values()
        .find(|f| {
            f.blocks
                .iter()
                .any(|b| matches!(b.terminator, Some(FirTerminator::Select { .. })))
        })
        .expect("worker select FIR");
    let select = worker
        .blocks
        .iter()
        .find_map(|block| match &block.terminator {
            Some(FirTerminator::Select { operation, cases }) => Some((*operation, cases)),
            _ => None,
        })
        .unwrap();
    assert_eq!(select.0, forge_frontend::RuntimeOperationId::SelectWait);
    assert_eq!(select.1.len(), 2);
    assert!(matches!(
        select.1[0],
        forge_frontend::FirSelectCase::Receive {
            operation: forge_frontend::RuntimeOperationId::ChannelReceive,
            ..
        }
    ));
    assert!(matches!(
        select.1[1],
        forge_frontend::FirSelectCase::Timeout {
            operation: forge_frontend::RuntimeOperationId::SelectTimeout,
            ..
        }
    ));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Const { value: forge_frontend::FirConst::Duration { value } }
            if value == "100ms"
    )));
}

#[test]
fn selected_receive_struct_pattern_binds_exact_payload_fields() {
    let output = lower(
        r#"
        module test.fir_select_pattern;
        struct Job { value: u32; }
        struct Jobs { marker: u8; }
        impl Jobs {
            fn recv(self: &Jobs) -> Job { return Job{value: 1u32}; }
        }
        fn worker(jobs: &Jobs) -> void {
            select {
                recv jobs -> Job{value} => { val copy: u32 = value; }
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(
        |op| matches!(op, FirInstructionKind::ExtractField { field, .. } if field == "value")
    ));
}

#[test]
fn raw_pointer_dereference_lowers_with_unsafe_provenance() {
    let output = lower(
        r#"
        module test.fir_raw_deref;
        fn read(p: *u32) -> u32 {
            unsafe { return *p; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::RawDeref {
                volatile: false,
                ..
            }
        }
    )));
}

#[test]
fn raw_pointer_store_uses_provenanced_raw_place() {
    let output = lower(
        r#"
        module test.fir_raw_store;
        fn write(p: *u32, value: u32) {
            unsafe { *p = value; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Store {
            place: forge_frontend::FirPlace::RawDeref { .. },
            ..
        }
    )));
}

#[test]
fn pointer_offset_is_a_dedicated_provenanced_fir_operation() {
    let output = lower(
        r#"
        module test.fir_pointer_offset;
        fn previous(p: *u32) -> *u32 {
            unsafe { return p - 2usize; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerOffset { subtract: true, .. }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Sub,
            ..
        }
    )));
}

#[test]
fn pointer_conversions_are_not_plain_fir_converts() {
    let output = lower(
        r#"
        module test.fir_pointer_convert;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { unsafe { return usize(p); } }
        fn cast(p: *u32) -> BytePtr { unsafe { return BytePtr(p); } }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerConvert {
            operation: forge_frontend::UnsafeOperationKind::PointerToInteger,
            ..
        }
    )));
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerConvert {
            operation: forge_frontend::UnsafeOperationKind::PointerReinterpret,
            ..
        }
    )));
}

#[test]
fn ordinary_reference_deref_remains_safe_fir_deref() {
    let output = lower(
        r#"
        module test.fir_reference_deref;
        fn read(p: &u32) -> u32 { return *p; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::Deref { .. }
        }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::RawDeref { .. }
        }
    )));
}
