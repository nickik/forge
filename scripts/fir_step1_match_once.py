from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing exact block: {label}")
    return text.replace(old, new, 1)


# ---------------------------------------------------------------------------
# Typed HIR: retain a semantic match plan. Step 1 intentionally plans only
# bool literal / wildcard patterns. Other pattern families stay source HIR and
# continue to be rejected at the FIR boundary until their numbered milestone.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()

marker = """#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDiagnostic {
"""
insert = """#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = \"test\", rename_all = \"snake_case\")]
pub enum MatchTest {
    Bool { value: bool },
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchArmPlan {
    pub test: MatchTest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub arms: Vec<TypedMatchArmPlan>,
}

"""
text = replace_once(text, marker, insert + marker, "match plan types")

old = """    ResolvedTry {
        source_error: Ty,
        target_error: Ty,
        hir: HirExpr,
    },
    OptionalPromote {
"""
new = """    ResolvedTry {
        source_error: Ty,
        target_error: Ty,
        hir: HirExpr,
    },
    ResolvedMatch {
        plan: TypedMatchPlan,
        hir: HirExpr,
    },
    OptionalPromote {
"""
text = replace_once(text, old, new, "ResolvedMatch kind")

old = """        let mut resolved_call: Option<ResolvedCallInfo> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;
        let mut ty = match &expr.kind {
"""
new = """        let mut resolved_call: Option<ResolvedCallInfo> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;
        let mut resolved_match: Option<TypedMatchPlan> = None;
        let mut ty = match &expr.kind {
"""
text = replace_once(text, old, new, "resolved_match local")

old = """                self.check_match_exhaustiveness(expr.span, &matched, arms);
                result
            }
"""
new = """                self.check_match_exhaustiveness(expr.span, &matched, arms);
                resolved_match = self.build_bool_match_plan(&matched, arms);
                result
            }
"""
text = replace_once(text, old, new, "build match plan")

old = """        } else if let Some(call) = resolved_call {
            TypedExprKind::ResolvedCall {
                target: call.target,
                method: call.method,
                receiver: call.receiver,
                argument_parameters: call.argument_parameters,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
"""
new = """        } else if let Some(call) = resolved_call {
            TypedExprKind::ResolvedCall {
                target: call.target,
                method: call.method,
                receiver: call.receiver,
                argument_parameters: call.argument_parameters,
                hir: expr.clone(),
            }
        } else if let Some(plan) = resolved_match {
            TypedExprKind::ResolvedMatch {
                plan,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
"""
text = replace_once(text, old, new, "retain match plan")

marker = """    fn check_match_exhaustiveness(
        &mut self,
"""
helper = """    fn build_bool_match_plan(
        &self,
        ty: &Ty,
        arms: &[crate::body_hir::HirMatchArm],
    ) -> Option<TypedMatchPlan> {
        if *ty != Ty::Bool {
            return None;
        }
        let mut planned = Vec::with_capacity(arms.len());
        for arm in arms {
            let test = match &arm.pattern.kind {
                HirPatternKind::Literal {
                    value: ast::PatternLiteral::Bool { value },
                } => MatchTest::Bool { value: *value },
                HirPatternKind::Wildcard => MatchTest::Always,
                _ => return None,
            };
            planned.push(TypedMatchArmPlan { test });
        }
        Some(TypedMatchPlan {
            scrutinee_type: Ty::Bool,
            arms: planned,
        })
    }

"""
text = replace_once(text, marker, helper + marker, "bool match plan helper")
path.write_text(text)


# ---------------------------------------------------------------------------
# FIR: consume the semantic plan. It never inspects the source pattern to
# decide what it means; HIR is used only for guard/body expressions and spans.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/fir_v1.rs")
text = path.read_text()
text = replace_once(
    text,
    """        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern,
        HirPatternKind, HirStmt, HirStmtKind,
""",
    """        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirMatchBody,
        HirPattern, HirPatternKind, HirStmt, HirStmtKind,
""",
    "FIR match body import",
)
text = replace_once(
    text,
    """        ConstValue, IntWidth, ResolvedReceiver, Ty, TypeCheckOutput, TypedBody, TypedExpr,
        TypedExprKind,
""",
    """        ConstValue, IntWidth, MatchTest, ResolvedReceiver, Ty, TypeCheckOutput, TypedBody,
        TypedExpr, TypedExprKind, TypedMatchPlan,
""",
    "FIR match plan imports",
)
text = replace_once(
    text,
    """    Const {
        value: FirConst,
    },
    FunctionRef {
""",
    """    Const {
        value: FirConst,
    },
    Unit,
    FunctionRef {
""",
    "FIR unit value",
)
text = replace_once(
    text,
    """            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                ..
            } => self.lower_try(expr, source_error, target_error, result_ty),
            TypedExprKind::Source { .. } => self.lower_source_expr(expr, result_ty),
""",
    """            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                ..
            } => self.lower_try(expr, source_error, target_error, result_ty),
            TypedExprKind::ResolvedMatch { plan, .. } => {
                self.lower_match(expr, plan, result_ty)
            }
            TypedExprKind::Source { .. } => self.lower_source_expr(expr, result_ty),
""",
    "ResolvedMatch FIR dispatch",
)

marker = """    fn lower_call_expression(&mut self, expr: &HirExpr, tail: bool) -> FirValueId {
"""
method = r'''    fn lower_match(
        &mut self,
        expr: &HirExpr,
        plan: &TypedMatchPlan,
        result_ty: Ty,
    ) -> FirValueId {
        let HirExprKind::Match { value, arms } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/match-shape",
                "resolved match plan is not attached to a match HIR node",
            );
            return self.poison(expr.span, result_ty);
        };
        if plan.scrutinee_type != Ty::Bool || plan.arms.len() != arms.len() {
            self.diagnostic(
                expr.span,
                "fir/match-plan",
                "typed boolean match plan does not match the source match shape",
            );
            return self.poison(expr.span, result_ty);
        }

        // Forge evaluation order requires the scrutinee to execute exactly once.
        let scrutinee = self.lower_expr(value);
        let result_local = if result_ty == Ty::Void {
            None
        } else {
            Some(self.synthetic_local(result_ty.clone()))
        };
        let join = self.new_block();

        for (arm, planned) in arms.iter().zip(&plan.arms) {
            let arm_entry = self.new_block();
            let next_arm = self.new_block();
            match planned.test {
                MatchTest::Bool { value: true } => {
                    self.terminate(FirTerminator::Branch {
                        condition: scrutinee,
                        then_block: arm_entry,
                        else_block: next_arm,
                    });
                }
                MatchTest::Bool { value: false } => {
                    let condition = self.emit_value(
                        arm.pattern.span,
                        Ty::Bool,
                        FirInstructionKind::Unary {
                            op: FirUnaryOp::Not,
                            value: scrutinee,
                        },
                    );
                    self.terminate(FirTerminator::Branch {
                        condition,
                        then_block: arm_entry,
                        else_block: next_arm,
                    });
                }
                MatchTest::Always => {
                    self.terminate(FirTerminator::Goto { target: arm_entry });
                }
            }

            self.switch_to(arm_entry);
            if let Some(guard) = &arm.guard {
                let guard_value = self.lower_expr(guard);
                let body_entry = self.new_block();
                self.terminate(FirTerminator::Branch {
                    condition: guard_value,
                    then_block: body_entry,
                    else_block: next_arm,
                });
                self.switch_to(body_entry);
            }

            match &arm.body {
                HirMatchBody::Expr(body) => {
                    let value = self.lower_expr(body);
                    if let Some(local) = result_local {
                        self.emit_void(
                            body.span,
                            FirInstructionKind::Store {
                                place: FirPlace::Local { local },
                                value,
                            },
                        );
                    }
                }
                HirMatchBody::Block(body) => self.lower_block(body),
            }
            if !self.terminated() {
                self.terminate(FirTerminator::Goto { target: join });
            }
            self.switch_to(next_arm);
        }

        // Semantic exhaustiveness guarantees that valid boolean matches cannot
        // fall through all unguarded coverage. Keep this explicit in FIR so a
        // broken semantic plan cannot become target-dependent behavior.
        if !self.terminated() {
            self.terminate(FirTerminator::Unreachable);
        }
        self.switch_to(join);
        if let Some(local) = result_local {
            self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::Load {
                    place: FirPlace::Local { local },
                },
            )
        } else {
            self.emit_value(expr.span, Ty::Void, FirInstructionKind::Unit)
        }
    }

'''
text = replace_once(text, marker, method + marker, "FIR boolean match lowerer")
path.write_text(text)


# ---------------------------------------------------------------------------
# Public debugging surface: expose the semantic match-plan types just like the
# other typed-HIR facts.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/lib.rs")
text = path.read_text()
text = replace_once(
    text,
    """    type_check_module, ConstValue, IntWidth, ResolvedReceiver, Ty, TypeCheckOutput, TypeDiagnostic,
    TypedBody, TypedExpr, TypedExprKind,
""",
    """    type_check_module, ConstValue, IntWidth, MatchTest, ResolvedReceiver, Ty, TypeCheckOutput,
    TypeDiagnostic, TypedBody, TypedExpr, TypedExprKind, TypedMatchArmPlan, TypedMatchPlan,
""",
    "lib typed match exports",
)
path.write_text(text)


# ---------------------------------------------------------------------------
# FIR tests: replace the expected-gap test with Step 1 acceptance coverage.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/tests/fir.rs")
text = path.read_text()
old = r'''#[test]
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
'''
new = r'''#[test]
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
    assert!(function.blocks.iter().all(|block| block.terminator.is_some()));
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
fn non_boolean_match_still_waits_for_later_pattern_steps() {
    let output = lower(
        r#"
        module test.fir_enum_match_later;
        enum Color { Red, Green }
        fn choose(value: Color) -> i32 {
            return match (value) { Color::Red => 1i32, Color::Green => 2i32, };
        }
        "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}
'''
text = replace_once(text, old, new, "FIR match tests")
path.write_text(text)


# Update the execution plan's completion record only for Step 1.
path = Path("docs/fir-completion-plan.md")
text = path.read_text()
text = text.replace(
    "Step 1: in progress. Later steps intentionally untouched.",
    "Step 1: implementation attempted for typed boolean/wildcard match plans and FIR CFG lowering. Later steps intentionally untouched.",
    1,
)
path.write_text(text)
