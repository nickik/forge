use forge_frontend::{lower_module, lower_resolved_bodies, parse_source, ResolvedBuiltinValue, ResolvedName};

#[test]
fn resolves_cosmic_sia32_privileged_source_names() {
    let source = r#"
        module test.sia_privileged_names;
        fn main() -> i32 {
            sia_trap(1u8);
            sia_swrite(5u8, 1u32);
            sia_tlbfence();
            return 0;
        }
    "#;
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "{:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(bodies.diagnostics.is_empty(), "{:?}", bodies.diagnostics);
    let body = bodies.functions.values().next().unwrap();
    let mut builtins = Vec::new();
    fn collect(expr: &forge_frontend::body_hir::HirExpr, out: &mut Vec<ResolvedBuiltinValue>) {
        use forge_frontend::body_hir::HirExprKind;
        match &expr.kind {
            HirExprKind::Call { callee, args } => {
                if let HirExprKind::Name { reference } = &callee.kind {
                    if let ResolvedName::BuiltinValue(value) = reference.root { out.push(value); }
                }
                for arg in args { collect(match arg { forge_frontend::body_hir::HirCallArg::Positional(v) => v, forge_frontend::body_hir::HirCallArg::Named { value, .. } => value }, out); }
            }
            _ => {}
        }
    }
    for stmt in &body.block.statements {
        if let forge_frontend::body_hir::HirStmtKind::Expr { value } = &stmt.kind { collect(value, &mut builtins); }
    }
    assert!(builtins.contains(&ResolvedBuiltinValue::SiaTrap));
    assert!(builtins.contains(&ResolvedBuiltinValue::SiaSwrite));
    assert!(builtins.contains(&ResolvedBuiltinValue::SiaTlbfence));
}
