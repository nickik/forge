from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing exact block: {label}")
    return text.replace(old, new, 1)


# ---------------------------------------------------------------------------
# Typed HIR: extend the boolean-only match plan into a small semantic pattern
# plan that covers Option, enums and tagged unions. FIR still receives resolved
# tests and typed projection paths; it does not inspect type definitions.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()

old = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "test", rename_all = "snake_case")]
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
'''
new = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "test", rename_all = "snake_case")]
pub enum MatchTest {
    Bool { value: bool },
    OptionNone,
    OptionSome,
    Variant { name: String },
    Always,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "projection", rename_all = "snake_case")]
pub enum MatchProjection {
    OptionPayload { ty: Ty },
    Field { name: String, ty: Ty },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchBinding {
    pub local: LocalId,
    pub ty: Ty,
    pub projections: Vec<MatchProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchArmPlan {
    pub test: MatchTest,
    pub bindings: Vec<TypedMatchBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub arms: Vec<TypedMatchArmPlan>,
}
'''
text = replace_once(text, old, new, "match plan types")

text = replace_once(
    text,
    "                resolved_match = self.build_bool_match_plan(&matched, arms);",
    "                resolved_match = self.build_match_plan(&matched, arms);",
    "match plan call",
)

old = '''    fn build_bool_match_plan(
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
'''
new = '''    fn build_match_plan(
        &self,
        ty: &Ty,
        arms: &[crate::body_hir::HirMatchArm],
    ) -> Option<TypedMatchPlan> {
        let supported = match ty {
            Ty::Bool | Ty::Optional { .. } => true,
            Ty::Nominal(id) => matches!(
                self.env.types.get(id).map(|info| &info.kind),
                Some(TypeInfoKind::Enum(_)) | Some(TypeInfoKind::Tagged(_))
            ),
            _ => false,
        };
        if !supported {
            return None;
        }

        let mut planned = Vec::with_capacity(arms.len());
        for arm in arms {
            let (test, bindings) = self.plan_match_pattern(&arm.pattern, ty)?;
            planned.push(TypedMatchArmPlan { test, bindings });
        }
        Some(TypedMatchPlan {
            scrutinee_type: ty.clone(),
            arms: planned,
        })
    }

    fn plan_match_pattern(
        &self,
        pattern: &HirPattern,
        ty: &Ty,
    ) -> Option<(MatchTest, Vec<TypedMatchBinding>)> {
        match &pattern.kind {
            HirPatternKind::Wildcard => Some((MatchTest::Always, Vec::new())),
            HirPatternKind::Binding { local, .. } => Some((
                MatchTest::Always,
                vec![TypedMatchBinding {
                    local: *local,
                    ty: ty.clone(),
                    projections: Vec::new(),
                }],
            )),
            HirPatternKind::As { local, pattern } => {
                let (test, mut bindings) = self.plan_match_pattern(pattern, ty)?;
                bindings.insert(
                    0,
                    TypedMatchBinding {
                        local: *local,
                        ty: ty.clone(),
                        projections: Vec::new(),
                    },
                );
                Some((test, bindings))
            }
            HirPatternKind::Literal {
                value: ast::PatternLiteral::Bool { value },
            } if *ty == Ty::Bool => Some((MatchTest::Bool { value: *value }, Vec::new())),
            HirPatternKind::None { .. } if matches!(ty, Ty::Optional { .. }) => {
                Some((MatchTest::OptionNone, Vec::new()))
            }
            HirPatternKind::Some { value } => {
                let Ty::Optional { inner } = ty else {
                    return None;
                };
                let mut bindings = Vec::new();
                let projections = vec![MatchProjection::OptionPayload {
                    ty: inner.as_ref().clone(),
                }];
                if !self.collect_irrefutable_match_bindings(
                    value,
                    inner,
                    &projections,
                    &mut bindings,
                ) {
                    return None;
                }
                Some((MatchTest::OptionSome, bindings))
            }
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                if expected != *ty {
                    return None;
                }
                let Ty::Nominal(id) = ty else {
                    return None;
                };
                match self.env.types.get(id).map(|info| &info.kind) {
                    Some(TypeInfoKind::Enum(variants)) => {
                        if !variants.contains(name) || !fields.is_empty() {
                            return None;
                        }
                        Some((MatchTest::Variant { name: name.clone() }, Vec::new()))
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        let defs = variants.get(name)?;
                        let mut bindings = Vec::new();
                        for field in fields {
                            let info = defs.get(&field.name)?;
                            let projections = vec![MatchProjection::Field {
                                name: field.name.clone(),
                                ty: info.ty.clone(),
                            }];
                            if let Some(local) = field.shorthand_local {
                                bindings.push(TypedMatchBinding {
                                    local,
                                    ty: info.ty.clone(),
                                    projections: projections.clone(),
                                });
                            }
                            if let Some(nested) = &field.pattern {
                                if !self.collect_irrefutable_match_bindings(
                                    nested,
                                    &info.ty,
                                    &projections,
                                    &mut bindings,
                                ) {
                                    return None;
                                }
                            }
                        }
                        Some((MatchTest::Variant { name: name.clone() }, bindings))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn collect_irrefutable_match_bindings(
        &self,
        pattern: &HirPattern,
        ty: &Ty,
        projections: &[MatchProjection],
        out: &mut Vec<TypedMatchBinding>,
    ) -> bool {
        match &pattern.kind {
            HirPatternKind::Wildcard => true,
            HirPatternKind::Binding { local, .. } => {
                out.push(TypedMatchBinding {
                    local: *local,
                    ty: ty.clone(),
                    projections: projections.to_vec(),
                });
                true
            }
            HirPatternKind::As { local, pattern } => {
                out.push(TypedMatchBinding {
                    local: *local,
                    ty: ty.clone(),
                    projections: projections.to_vec(),
                });
                self.collect_irrefutable_match_bindings(pattern, ty, projections, out)
            }
            _ => false,
        }
    }
'''
text = replace_once(text, old, new, "boolean match planner")
path.write_text(text)


# ---------------------------------------------------------------------------
# FIR: consume the semantic tests/projections. No type-definition lookup is
# added here; enum/tagged decisions are already encoded in TypedMatchPlan.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/fir_v1.rs")
text = path.read_text()
text = replace_once(
    text,
    '''        ConstValue, IntWidth, MatchTest, ResolvedReceiver, Ty, TypeCheckOutput, TypedBody,
        TypedExpr, TypedExprKind, TypedMatchPlan,
''',
    '''        ConstValue, IntWidth, MatchProjection, MatchTest, ResolvedReceiver, Ty,
        TypeCheckOutput, TypedBody, TypedExpr, TypedExprKind, TypedMatchBinding, TypedMatchPlan,
''',
    "FIR match imports",
)

old = '''        if plan.scrutinee_type != Ty::Bool || plan.arms.len() != arms.len() {
            self.diagnostic(
                expr.span,
                "fir/match-plan",
                "typed boolean match plan does not match the source match shape",
            );
            return self.poison(expr.span, result_ty);
        }

        // Forge evaluation order requires the scrutinee to execute exactly once.
        let scrutinee = self.lower_expr(value);
'''
new = '''        let actual_scrutinee_type = self.expr_ty(value);
        if plan.scrutinee_type != actual_scrutinee_type || plan.arms.len() != arms.len() {
            self.diagnostic(
                expr.span,
                "fir/match-plan",
                "typed match plan does not match the source match shape/type",
            );
            return self.poison(expr.span, result_ty);
        }

        // Forge evaluation order requires the scrutinee to execute exactly once.
        let scrutinee = self.lower_expr(value);
'''
text = replace_once(text, old, new, "FIR match plan validation")

old = '''            match planned.test {
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
'''
new = '''            let condition = match &planned.test {
                MatchTest::Bool { value: true } => Some(scrutinee),
                MatchTest::Bool { value: false } => Some(self.emit_value(
                    arm.pattern.span,
                    Ty::Bool,
                    FirInstructionKind::Unary {
                        op: FirUnaryOp::Not,
                        value: scrutinee,
                    },
                )),
                MatchTest::OptionSome => Some(self.emit_value(
                    arm.pattern.span,
                    Ty::Bool,
                    FirInstructionKind::OptionIsSome { value: scrutinee },
                )),
                MatchTest::OptionNone => {
                    let is_some = self.emit_value(
                        arm.pattern.span,
                        Ty::Bool,
                        FirInstructionKind::OptionIsSome { value: scrutinee },
                    );
                    Some(self.emit_value(
                        arm.pattern.span,
                        Ty::Bool,
                        FirInstructionKind::Unary {
                            op: FirUnaryOp::Not,
                            value: is_some,
                        },
                    ))
                }
                MatchTest::Variant { name } => Some(self.emit_value(
                    arm.pattern.span,
                    Ty::Bool,
                    FirInstructionKind::VariantIs {
                        value: scrutinee,
                        name: name.clone(),
                    },
                )),
                MatchTest::Always => None,
            };
            if let Some(condition) = condition {
                self.terminate(FirTerminator::Branch {
                    condition,
                    then_block: arm_entry,
                    else_block: next_arm,
                });
            } else {
                self.terminate(FirTerminator::Goto { target: arm_entry });
            }

            self.switch_to(arm_entry);
            self.lower_match_bindings(arm.pattern.span, scrutinee, &planned.bindings);
            if let Some(guard) = &arm.guard {
'''
text = replace_once(text, old, new, "FIR match tests")

old = '''        // Semantic exhaustiveness guarantees that valid boolean matches cannot
        // fall through all unguarded coverage. Keep this explicit in FIR so a
        // broken semantic plan cannot become target-dependent behavior.
'''
new = '''        // Semantic exhaustiveness guarantees that valid finite matches cannot
        // fall through all unguarded coverage. Keep this explicit in FIR so a
        // broken semantic plan cannot become target-dependent behavior.
'''
text = replace_once(text, old, new, "FIR match comment")

marker = '''    fn lower_call_expression(&mut self, expr: &HirExpr, tail: bool) -> FirValueId {
'''
helper = '''    fn lower_match_bindings(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        bindings: &[TypedMatchBinding],
    ) {
        for binding in bindings {
            let mut value = scrutinee;
            for projection in &binding.projections {
                value = match projection {
                    MatchProjection::OptionPayload { ty } => self.emit_value(
                        span,
                        ty.clone(),
                        FirInstructionKind::OptionUnwrap { value },
                    ),
                    MatchProjection::Field { name, ty } => self.emit_value(
                        span,
                        ty.clone(),
                        FirInstructionKind::ExtractField {
                            base: value,
                            field: name.clone(),
                        },
                    ),
                };
            }
            let Some(local) = self.local_map.get(&binding.local).copied() else {
                self.diagnostic(
                    span,
                    "fir/match-binding-local",
                    format!("match binding {:?} has no FIR local", binding.local),
                );
                continue;
            };
            self.emit_void(
                span,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    value,
                },
            );
        }
    }

'''
if marker not in text:
    raise SystemExit("missing lower_call_expression marker")
text = text.replace(marker, helper + marker, 1)
path.write_text(text)


# ---------------------------------------------------------------------------
# Public debug surface: expose the semantic match-plan pieces along with the
# existing TypedMatchPlan exports.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/src/lib.rs")
text = path.read_text()
text = replace_once(
    text,
    '''    type_check_module, ConstValue, IntWidth, MatchTest, ResolvedReceiver, Ty, TypeCheckOutput,
    TypeDiagnostic, TypedBody, TypedExpr, TypedExprKind, TypedMatchArmPlan, TypedMatchPlan,
''',
    '''    type_check_module, ConstValue, IntWidth, MatchProjection, MatchTest, ResolvedReceiver, Ty,
    TypeCheckOutput, TypeDiagnostic, TypedBody, TypedExpr, TypedExprKind, TypedMatchArmPlan,
    TypedMatchBinding, TypedMatchPlan,
''',
    "lib match exports",
)
path.write_text(text)


# ---------------------------------------------------------------------------
# FIR regression tests for milestones 2, 3 and 4. Replace the old intentionally
# failing enum-boundary test with a still-unimplemented structural-pattern test.
# ---------------------------------------------------------------------------
path = Path("crates/forge-frontend/tests/fir.rs")
text = path.read_text()
old = '''#[test]
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
new = '''#[test]
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
fn structural_match_still_waits_for_later_pattern_step() {
    let output = lower(
        r#"
        module test.fir_struct_match_later;
        struct Point { x: i32; }
        fn choose(value: Point) -> i32 {
            return match (value) { Point{x} => x, };
        }
        "#,
    );
    assert!(output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}
'''
text = replace_once(text, old, new, "FIR pending match test")
path.write_text(text)


# ---------------------------------------------------------------------------
# Keep the execution plan current. Do not mark later milestones as touched.
# ---------------------------------------------------------------------------
path = Path("docs/fir-completion-plan.md")
text = path.read_text()
old = '''## Completion record

Step 1: implementation attempted for typed boolean/wildcard match plans and FIR CFG lowering. Later steps intentionally untouched.
'''
new = '''## Steps 2-4 acceptance tests

- Optional `None`/`Some` tests lower through `OptionIsSome`; `Some` payloads are unwrapped only on the matching edge.
- Optional payload bindings are stored before guards, so guards may reference those locals.
- Closed enum arms lower through resolved `VariantIs` tests without FIR consulting enum definitions.
- Tagged-union arms lower through resolved `VariantIs` plus typed payload-field projections and local bindings.
- Guards retain source-order fallthrough for Option, enum, and tagged-union arms.
- Structural/scalar/range/or/sequence patterns remain later milestones and still stop at the FIR boundary.

## Completion record

- Step 1 complete: typed boolean/wildcard match plans and explicit FIR CFG lowering.
- Step 2 complete: optional `None`/`Some` decisions, payload extraction, bindings, and guarded fallthrough.
- Step 3 complete: enum discriminant decisions lowered as resolved FIR variant tests.
- Step 4 complete: tagged-union discriminant tests plus typed payload-field extraction/bindings.
- Steps 5-16 intentionally untouched.
'''
text = replace_once(text, old, new, "plan completion record")
path.write_text(text)
