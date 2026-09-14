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
fn fir_reports_missing_pre_fir_pattern_decision_tree() {
    let output = lower(
        r#"
        module test.fir_match_gap;
        fn choose(value: bool) -> i32 {
            return match (value) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}
