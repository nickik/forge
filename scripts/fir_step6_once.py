from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


tc = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = tc.read_text()
text = text.replace(
'''    Variant {
        name: String,
    },
}''',
'''    Variant {
        name: String,
    },
    Length {
        count: u64,
        at_least: bool,
    },
}''',
1,
)
text = text.replace(
'''pub enum MatchProjection {
    OptionPayload { ty: Ty },
    Field { name: String, ty: Ty },
}''',
'''pub enum MatchProjection {
    OptionPayload { ty: Ty },
    Field { name: String, ty: Ty },
    Index { index: u64, ty: Ty },
    Rest { start: u64, ty: Ty },
}''',
1,
)
text = text.replace(
'''pub enum MatchCondition {
    Always,
    Test {''',
'''pub enum MatchCondition {
    Always,
    Never,
    Test {''',
1,
)
old = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchArmPlan {
    pub condition: MatchCondition,
    pub bindings: Vec<TypedMatchBinding>,
}
'''
new = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchAlternative {
    pub condition: MatchCondition,
    pub bindings: Vec<TypedMatchBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedMatchArmPlan {
    pub alternatives: Vec<TypedMatchAlternative>,
}
'''
if old not in text:
    raise SystemExit("typed arm plan not found")
text = text.replace(old, new, 1)

start = text.index("    fn build_match_plan(")
end = text.index("    fn match_scalar(", start)
replacement = r'''    fn build_match_plan(
        &self,
        ty: &Ty,
        arms: &[crate::body_hir::HirMatchArm],
    ) -> Option<TypedMatchPlan> {
        let supported = match ty {
            Ty::Bool
            | Ty::Char
            | Ty::Str
            | Ty::Int { .. }
            | Ty::Optional { .. }
            | Ty::Array { .. }
            | Ty::Slice { .. } => true,
            Ty::Nominal(id) => matches!(
                self.env.types.get(id).map(|info| &info.kind),
                Some(TypeInfoKind::Struct(_))
                    | Some(TypeInfoKind::Enum(_))
                    | Some(TypeInfoKind::Tagged(_))
            ),
            _ => false,
        };
        if !supported {
            return None;
        }

        let mut planned = Vec::with_capacity(arms.len());
        for arm in arms {
            planned.push(TypedMatchArmPlan {
                alternatives: self.plan_match_pattern(&arm.pattern, ty)?,
            });
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
    ) -> Option<Vec<TypedMatchAlternative>> {
        self.plan_match_pattern_at(pattern, ty, &[])
    }

    fn plan_match_pattern_at(
        &self,
        pattern: &HirPattern,
        ty: &Ty,
        projections: &[MatchProjection],
    ) -> Option<Vec<TypedMatchAlternative>> {
        let test = |test| MatchCondition::Test {
            projections: projections.to_vec(),
            test,
        };
        let one = |condition, bindings| {
            vec![TypedMatchAlternative {
                condition,
                bindings,
            }]
        };
        match &pattern.kind {
            HirPatternKind::Wildcard => Some(one(MatchCondition::Always, Vec::new())),
            HirPatternKind::Binding { local, .. } => Some(one(
                MatchCondition::Always,
                vec![TypedMatchBinding {
                    local: *local,
                    ty: ty.clone(),
                    projections: projections.to_vec(),
                }],
            )),
            HirPatternKind::As { local, pattern } => {
                let mut alternatives = self.plan_match_pattern_at(pattern, ty, projections)?;
                for alternative in &mut alternatives {
                    alternative.bindings.insert(
                        0,
                        TypedMatchBinding {
                            local: *local,
                            ty: ty.clone(),
                            projections: projections.to_vec(),
                        },
                    );
                }
                Some(alternatives)
            }
            HirPatternKind::Literal {
                value: ast::PatternLiteral::Bool { value },
            } if *ty == Ty::Bool => Some(one(MatchCondition::Test {
                projections: projections.to_vec(),
                test: MatchTest::Bool { value: *value },
            }, Vec::new())),
            HirPatternKind::Literal { value } => self.match_scalar(value, ty).map(|value| {
                one(
                    test(MatchTest::ScalarLiteral { value }),
                    Vec::new(),
                )
            }),
            HirPatternKind::Range {
                start,
                end,
                inclusive,
            } => {
                let start = self.match_scalar(start, ty)?;
                let end = self.match_scalar(end, ty)?;
                Some(one(
                    test(MatchTest::ScalarRange {
                        start,
                        end,
                        inclusive: *inclusive,
                    }),
                    Vec::new(),
                ))
            }
            HirPatternKind::None { .. } if matches!(ty, Ty::Optional { .. }) => {
                Some(one(test(MatchTest::OptionNone), Vec::new()))
            }
            HirPatternKind::Some { value } => {
                let Ty::Optional { inner } = ty else {
                    return None;
                };
                let mut payload = projections.to_vec();
                payload.push(MatchProjection::OptionPayload {
                    ty: inner.as_ref().clone(),
                });
                let nested = self.plan_match_pattern_at(value, inner, &payload)?;
                Some(
                    nested
                        .into_iter()
                        .map(|alternative| TypedMatchAlternative {
                            condition: self.match_condition_all(vec![
                                test(MatchTest::OptionSome),
                                alternative.condition,
                            ]),
                            bindings: alternative.bindings,
                        })
                        .collect(),
                )
            }
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                if expected != *ty {
                    return None;
                }
                let Ty::Nominal(id) = ty else {
                    return None;
                };
                let Some(TypeInfoKind::Struct(defs)) =
                    self.env.types.get(id).map(|info| &info.kind)
                else {
                    return None;
                };
                self.plan_record_fields(
                    vec![TypedMatchAlternative {
                        condition: MatchCondition::Always,
                        bindings: Vec::new(),
                    }],
                    fields,
                    defs,
                    projections,
                )
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
                let variant_test = test(MatchTest::Variant { name: name.clone() });
                match self.env.types.get(id).map(|info| &info.kind) {
                    Some(TypeInfoKind::Enum(variants)) => {
                        if !variants.contains(name) || !fields.is_empty() {
                            return None;
                        }
                        Some(one(variant_test, Vec::new()))
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        let defs = variants.get(name)?;
                        self.plan_record_fields(
                            one(variant_test, Vec::new()),
                            fields,
                            defs,
                            projections,
                        )
                    }
                    _ => None,
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let (element, length_condition) = match ty {
                    Ty::Slice { element, .. } => (
                        element.as_ref().clone(),
                        MatchCondition::Test {
                            projections: projections.to_vec(),
                            test: MatchTest::Length {
                                count: items.len() as u64,
                                at_least: rest.is_some(),
                            },
                        },
                    ),
                    Ty::Array {
                        element,
                        length: Some(length),
                    } => {
                        let compatible = if rest.is_some() {
                            *length >= items.len() as u64
                        } else {
                            *length == items.len() as u64
                        };
                        (
                            element.as_ref().clone(),
                            if compatible {
                                MatchCondition::Always
                            } else {
                                MatchCondition::Never
                            },
                        )
                    }
                    Ty::Array { element, .. } => (
                        element.as_ref().clone(),
                        MatchCondition::Test {
                            projections: projections.to_vec(),
                            test: MatchTest::Length {
                                count: items.len() as u64,
                                at_least: rest.is_some(),
                            },
                        },
                    ),
                    _ => return None,
                };
                let mut alternatives = one(length_condition, Vec::new());
                for (index, item) in items.iter().enumerate() {
                    let mut item_projection = projections.to_vec();
                    item_projection.push(MatchProjection::Index {
                        index: index as u64,
                        ty: element.clone(),
                    });
                    let nested = self.plan_match_pattern_at(item, &element, &item_projection)?;
                    alternatives = self.combine_match_alternatives(alternatives, nested);
                }
                if let Some(local) = rest {
                    let rest_ty = self.sequence_rest_type(ty, items.len() as u64);
                    for alternative in &mut alternatives {
                        let mut rest_projection = projections.to_vec();
                        rest_projection.push(MatchProjection::Rest {
                            start: items.len() as u64,
                            ty: rest_ty.clone(),
                        });
                        alternative.bindings.push(TypedMatchBinding {
                            local: *local,
                            ty: rest_ty.clone(),
                            projections: rest_projection,
                        });
                    }
                }
                Some(alternatives)
            }
            HirPatternKind::Or { patterns } => {
                let mut alternatives = Vec::new();
                for pattern in patterns {
                    alternatives.extend(self.plan_match_pattern_at(pattern, ty, projections)?);
                }
                Some(alternatives)
            }
            HirPatternKind::Map { .. } => None,
        }
    }

    fn plan_record_fields(
        &self,
        mut alternatives: Vec<TypedMatchAlternative>,
        fields: &[crate::body_hir::HirPatternField],
        defs: &BTreeMap<String, FieldInfo>,
        projections: &[MatchProjection],
    ) -> Option<Vec<TypedMatchAlternative>> {
        for field in fields {
            let info = defs.get(&field.name)?;
            let mut field_projection = projections.to_vec();
            field_projection.push(MatchProjection::Field {
                name: field.name.clone(),
                ty: info.ty.clone(),
            });
            if let Some(local) = field.shorthand_local {
                for alternative in &mut alternatives {
                    alternative.bindings.push(TypedMatchBinding {
                        local,
                        ty: info.ty.clone(),
                        projections: field_projection.clone(),
                    });
                }
            }
            if let Some(pattern) = &field.pattern {
                let nested =
                    self.plan_match_pattern_at(pattern, &info.ty, &field_projection)?;
                alternatives = self.combine_match_alternatives(alternatives, nested);
            }
        }
        Some(alternatives)
    }

    fn combine_match_alternatives(
        &self,
        left: Vec<TypedMatchAlternative>,
        right: Vec<TypedMatchAlternative>,
    ) -> Vec<TypedMatchAlternative> {
        let mut combined = Vec::new();
        for left in left {
            for right in &right {
                let mut bindings = left.bindings.clone();
                bindings.extend(right.bindings.clone());
                combined.push(TypedMatchAlternative {
                    condition: self.match_condition_all(vec![
                        left.condition.clone(),
                        right.condition.clone(),
                    ]),
                    bindings,
                });
            }
        }
        combined
    }

    fn match_condition_all(&self, conditions: Vec<MatchCondition>) -> MatchCondition {
        let mut flattened = Vec::new();
        for condition in conditions {
            match condition {
                MatchCondition::Never => return MatchCondition::Never,
                MatchCondition::Always => {}
                MatchCondition::All { conditions } => flattened.extend(conditions),
                other => flattened.push(other),
            }
        }
        match flattened.len() {
            0 => MatchCondition::Always,
            1 => flattened.pop().expect("one match condition"),
            _ => MatchCondition::All {
                conditions: flattened,
            },
        }
    }

    fn sequence_rest_type(&self, ty: &Ty, start: u64) -> Ty {
        match ty {
            Ty::Slice { mutable, element } => Ty::Slice {
                mutable: *mutable,
                element: element.clone(),
            },
            Ty::Array { element, length } => Ty::Array {
                element: element.clone(),
                length: length.map(|length| length.saturating_sub(start)),
            },
            _ => Ty::Error,
        }
    }

'''
text = text[:start] + replacement + text[end:]

# Remove the old narrow irrefutable-only match binding planner.
start = text.index("    fn collect_irrefutable_match_bindings(")
end = text.index("    fn check_match_exhaustiveness(", start)
text = text[:start] + text[end:]

# Rest bindings in semantic local typing get the actual tail type for fixed arrays.
old = '''                if let Some(id) = rest {
                    out.insert(*id, ty.clone());
                }
'''
new = '''                if let Some(id) = rest {
                    out.insert(*id, self.sequence_rest_type(ty, items.len() as u64));
                }
'''
if old not in text:
    raise SystemExit("sequence rest typing site not found")
text = text.replace(old, new, 1)
tc.write_text(text)

fir = Path("crates/forge-frontend/src/fir_v1.rs")
text = fir.read_text()
# Add a target-independent sequence tail operation.
text = text.replace(
'''    IndexUnchecked {
        base: FirValueId,
        index: FirValueId,
    },
    AddressOf {''',
'''    IndexUnchecked {
        base: FirValueId,
        index: FirValueId,
    },
    Subsequence {
        base: FirValueId,
        start: u64,
    },
    AddressOf {''',
1,
)

start = text.index("        for (arm, planned) in arms.iter().zip(&plan.arms) {")
end = text.index("\n        // Semantic exhaustiveness guarantees", start)
replacement = r'''        for (arm, planned) in arms.iter().zip(&plan.arms) {
            let next_arm = self.new_block();
            let matched_entry = self.new_block();
            if planned.alternatives.is_empty() {
                self.terminate(FirTerminator::Goto { target: next_arm });
            } else {
                for (index, alternative) in planned.alternatives.iter().enumerate() {
                    let binding_entry = self.new_block();
                    let false_target = if index + 1 == planned.alternatives.len() {
                        next_arm
                    } else {
                        self.new_block()
                    };
                    let condition = self.lower_match_condition(
                        arm.pattern.span,
                        scrutinee,
                        &plan.scrutinee_type,
                        &alternative.condition,
                    );
                    if let Some(condition) = condition {
                        self.terminate(FirTerminator::Branch {
                            condition,
                            then_block: binding_entry,
                            else_block: false_target,
                        });
                    } else {
                        self.terminate(FirTerminator::Goto {
                            target: binding_entry,
                        });
                    }

                    self.switch_to(binding_entry);
                    self.lower_match_bindings(
                        arm.pattern.span,
                        scrutinee,
                        &alternative.bindings,
                    );
                    if !self.terminated() {
                        self.terminate(FirTerminator::Goto {
                            target: matched_entry,
                        });
                    }
                    if index + 1 != planned.alternatives.len() {
                        self.switch_to(false_target);
                    }
                }
            }

            self.switch_to(matched_entry);
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
'''
text = text[:start] + replacement + text[end:]

start = text.index("    fn lower_match_condition(")
end = text.index("    fn lower_match_test(", start)
replacement = r'''    fn lower_match_condition(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        scrutinee_ty: &Ty,
        condition: &MatchCondition,
    ) -> Option<FirValueId> {
        match condition {
            MatchCondition::Always => None,
            MatchCondition::Never => Some(self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::Const {
                    value: FirConst::Bool { value: false },
                },
            )),
            MatchCondition::Test { projections, test } => {
                let value = self.lower_match_projection(span, scrutinee, projections);
                let value_ty = projections
                    .last()
                    .map(|projection| match projection {
                        MatchProjection::OptionPayload { ty }
                        | MatchProjection::Field { ty, .. }
                        | MatchProjection::Index { ty, .. }
                        | MatchProjection::Rest { ty, .. } => ty.clone(),
                    })
                    .unwrap_or_else(|| scrutinee_ty.clone());
                Some(self.lower_match_test(span, value, &value_ty, test))
            }
            MatchCondition::All { conditions } => self.lower_match_condition_list(
                span,
                scrutinee,
                scrutinee_ty,
                conditions,
                true,
            ),
            MatchCondition::Any { conditions } => self.lower_match_condition_list(
                span,
                scrutinee,
                scrutinee_ty,
                conditions,
                false,
            ),
        }
    }

    fn lower_match_condition_list(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        scrutinee_ty: &Ty,
        conditions: &[MatchCondition],
        all: bool,
    ) -> Option<FirValueId> {
        let mut terminal_block = None;
        let mut join_block = None;
        let mut result_local = None;
        let mut saw_test = false;

        for condition in conditions {
            let condition = self.lower_match_condition(span, scrutinee, scrutinee_ty, condition);
            let Some(condition) = condition else {
                if all {
                    continue;
                }
                return None;
            };
            saw_test = true;
            let terminal = *terminal_block.get_or_insert_with(|| self.new_block());
            let join = *join_block.get_or_insert_with(|| self.new_block());
            let local = *result_local.get_or_insert_with(|| self.synthetic_local(Ty::Bool));
            let next = self.new_block();
            self.terminate(if all {
                FirTerminator::Branch {
                    condition,
                    then_block: next,
                    else_block: terminal,
                }
            } else {
                FirTerminator::Branch {
                    condition,
                    then_block: terminal,
                    else_block: next,
                }
            });
            self.switch_to(next);
            let _ = (join, local);
        }

        if !saw_test {
            return if all {
                None
            } else {
                Some(self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Const {
                        value: FirConst::Bool { value: false },
                    },
                ))
            };
        }

        let terminal = terminal_block.expect("match condition terminal block");
        let join = join_block.expect("match condition join block");
        let local = result_local.expect("match condition result local");
        let fallthrough_value = self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Const {
                value: FirConst::Bool { value: all },
            },
        );
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local },
                value: fallthrough_value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });

        self.switch_to(terminal);
        let terminal_value = self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Const {
                value: FirConst::Bool { value: !all },
            },
        );
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local },
                value: terminal_value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });

        self.switch_to(join);
        Some(self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Load {
                place: FirPlace::Local { local },
            },
        ))
    }

'''
text = text[:start] + replacement + text[end:]

# Add length tests.
old = '''            MatchTest::Variant { name } => self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::VariantIs {
                    value,
                    name: name.clone(),
                },
            ),
        }
    }
'''
new = '''            MatchTest::Variant { name } => self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::VariantIs {
                    value,
                    name: name.clone(),
                },
            ),
            MatchTest::Length { count, at_least } => {
                let len = self.emit_value(span, usize_ty(), FirInstructionKind::Len { value });
                let expected = self.emit_value(
                    span,
                    usize_ty(),
                    FirInstructionKind::Const {
                        value: FirConst::Integer {
                            text: count.to_string(),
                        },
                    },
                );
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: if *at_least {
                            BinaryOp::GreaterEq
                        } else {
                            BinaryOp::Eq
                        },
                        overflow: None,
                        left: len,
                        right: expected,
                    },
                )
            }
        }
    }
'''
if old not in text:
    raise SystemExit("match test variant tail not found")
text = text.replace(old, new, 1)

# Expand projection lowering for sequence items/rest.
old = '''                MatchProjection::Field { name, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::ExtractField {
                        base: value,
                        field: name.clone(),
                    },
                ),
            };
'''
new = '''                MatchProjection::Field { name, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::ExtractField {
                        base: value,
                        field: name.clone(),
                    },
                ),
                MatchProjection::Index { index, ty } => {
                    let index = self.emit_value(
                        span,
                        usize_ty(),
                        FirInstructionKind::Const {
                            value: FirConst::Integer {
                                text: index.to_string(),
                            },
                        },
                    );
                    self.emit_value(
                        span,
                        ty.clone(),
                        FirInstructionKind::IndexUnchecked { base: value, index },
                    )
                }
                MatchProjection::Rest { start, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::Subsequence {
                        base: value,
                        start: *start,
                    },
                ),
            };
'''
if old not in text:
    raise SystemExit("projection field tail not found")
text = text.replace(old, new, 1)
fir.write_text(text)

# Export the new alternative plan type.
lib = Path("crates/forge-frontend/src/lib.rs")
text = lib.read_text()
text = text.replace(
    "TypedExpr, TypedExprKind, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan,",
    "TypedExpr, TypedExprKind, TypedMatchAlternative, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan,",
    1,
)
lib.write_text(text)

# Focused Step 6 FIR tests. Replace the old boundary-negative test with map-pattern deferral.
tests = Path("crates/forge-frontend/tests/fir.rs")
text = tests.read_text()
old_start = text.index("#[test]\nfn structural_match_still_waits_for_later_pattern_step()")
old_end = text.find("\n#[test]", old_start + 8)
if old_end == -1:
    old_end = len(text)
replacement = r'''#[test]
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
'''
text = text[:old_start] + replacement + text[old_end:]
text += r'''

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
    assert!(instructions(&output).filter(|op| matches!(op, FirInstructionKind::Store { .. })).count() >= 3);
}
'''
tests.write_text(text)

plan = Path("docs/fir-completion-plan.md")
text = plan.read_text().replace(
    "- Steps 6-16 intentionally untouched.",
    "- Step 6 complete: struct, sequence/rest, as, and OR patterns lower through typed alternatives/projections with short-circuit-safe structural tests.\n- Steps 7-16 intentionally untouched.",
    1,
)
plan.write_text(text)

Path("scripts/fir_step6_once.py").unlink()
Path(".github/workflows/fir-step6-once.yml").unlink()
