from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:140]!r}")
    p.write_text(text.replace(old, new, 1))


tc = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = tc.read_text()

# Retain the finite root domain on the same semantic plan FIR consumes.
replace_once(
    str(tc),
    '''pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub arms: Vec<TypedMatchArmPlan>,
}
''',
    '''pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub finite_cases: Option<Vec<String>>,
    pub arms: Vec<TypedMatchArmPlan>,
}
''',
)
text = tc.read_text()
old = '''        Some(TypedMatchPlan {
            scrutinee_type: ty.clone(),
            arms: planned,
        })
'''
new = '''        Some(TypedMatchPlan {
            scrutinee_type: ty.clone(),
            finite_cases: self
                .finite_match_cases(ty)
                .map(|cases| cases.into_iter().collect()),
            arms: planned,
        })
'''
if old not in text:
    raise SystemExit("typed match plan construction not found")
text = text.replace(old, new, 1)

# Planned patterns use the exact decision representation for both usefulness and FIR.
old = '''                self.check_match_exhaustiveness(expr.span, &matched, arms);
                resolved_match = self.build_match_plan(&matched, arms);
                result
'''
new = '''                if let Some(plan) = self.build_match_plan(&matched, arms) {
                    self.check_match_plan_usefulness(expr.span, arms, &plan);
                    resolved_match = Some(plan);
                } else {
                    // Unsupported collection/map protocol patterns still use the
                    // conservative legacy finite-domain check until Step 14.
                    self.check_match_exhaustiveness(expr.span, &matched, arms);
                }
                result
'''
if old not in text:
    raise SystemExit("match exhaustiveness call site not found")
text = text.replace(old, new, 1)

# Insert semantic-plan usefulness/exhaustiveness analysis immediately before the
# conservative legacy checker retained only for unsupported pattern protocols.
marker = "    fn check_match_exhaustiveness(\n"
pos = text.index(marker)
helpers = r'''    fn check_match_plan_usefulness(
        &mut self,
        span: Span,
        arms: &[crate::body_hir::HirMatchArm],
        plan: &TypedMatchPlan,
    ) {
        let mut prior_unguarded = Vec::<MatchCondition>::new();

        for (arm, planned) in arms.iter().zip(&plan.arms) {
            let unreachable = !planned.alternatives.is_empty()
                && planned.alternatives.iter().all(|alternative| {
                    prior_unguarded.iter().any(|earlier| {
                        self.match_condition_subsumes(earlier, &alternative.condition)
                    })
                });
            if unreachable {
                self.diagnostic(
                    arm.pattern.span,
                    "match/unreachable-arm",
                    "match arm is unreachable because earlier unguarded arms cover all of its alternatives",
                );
            }
            if arm.guard.is_none() {
                prior_unguarded.extend(
                    planned
                        .alternatives
                        .iter()
                        .map(|alternative| alternative.condition.clone()),
                );
            }
        }

        let Some(finite_cases) = &plan.finite_cases else {
            return;
        };
        let required = finite_cases.iter().cloned().collect::<BTreeSet<_>>();
        let mut covered = BTreeSet::new();
        for (arm, planned) in arms.iter().zip(&plan.arms) {
            if arm.guard.is_some() {
                continue;
            }
            for alternative in &planned.alternatives {
                covered.extend(self.match_condition_guaranteed_cases(
                    &alternative.condition,
                    &required,
                ));
            }
        }
        let missing = required.difference(&covered).cloned().collect::<Vec<_>>();
        if !missing.is_empty() {
            self.diagnostic(
                span,
                "match/non-exhaustive",
                format!("non-exhaustive match; missing {}", missing.join(", ")),
            );
        }
    }

    fn match_condition_subsumes(
        &self,
        earlier: &MatchCondition,
        later: &MatchCondition,
    ) -> bool {
        match earlier {
            MatchCondition::Always => true,
            MatchCondition::Never => matches!(later, MatchCondition::Never),
            MatchCondition::Test { projections, test } => {
                self.match_condition_implies_test(later, projections, test)
            }
            MatchCondition::All { conditions } => conditions
                .iter()
                .all(|condition| self.match_condition_subsumes(condition, later)),
            MatchCondition::Any { conditions } => conditions
                .iter()
                .any(|condition| self.match_condition_subsumes(condition, later)),
        }
    }

    fn match_condition_implies_test(
        &self,
        later: &MatchCondition,
        earlier_projections: &[MatchProjection],
        earlier_test: &MatchTest,
    ) -> bool {
        match later {
            MatchCondition::Never => true,
            MatchCondition::Always => false,
            MatchCondition::Test { projections, test } => {
                projections == earlier_projections && self.match_test_implies(test, earlier_test)
            }
            MatchCondition::All { conditions } => conditions.iter().any(|condition| {
                self.match_condition_implies_test(
                    condition,
                    earlier_projections,
                    earlier_test,
                )
            }),
            MatchCondition::Any { conditions } => conditions.iter().all(|condition| {
                self.match_condition_implies_test(
                    condition,
                    earlier_projections,
                    earlier_test,
                )
            }),
        }
    }

    // `later` implies `earlier`: the later test is equal or more specific.
    fn match_test_implies(&self, later: &MatchTest, earlier: &MatchTest) -> bool {
        if later == earlier {
            return true;
        }
        match (later, earlier) {
            (
                MatchTest::ScalarLiteral { value },
                MatchTest::ScalarRange {
                    start,
                    end,
                    inclusive,
                },
            ) => self.scalar_in_range(value, start, end, *inclusive),
            (
                MatchTest::ScalarRange {
                    start: later_start,
                    end: later_end,
                    inclusive: later_inclusive,
                },
                MatchTest::ScalarRange {
                    start: earlier_start,
                    end: earlier_end,
                    inclusive: earlier_inclusive,
                },
            ) => {
                let Some(start_order) = self.match_scalar_cmp(later_start, earlier_start) else {
                    return false;
                };
                let Some(end_order) = self.match_scalar_cmp(later_end, earlier_end) else {
                    return false;
                };
                start_order != std::cmp::Ordering::Less
                    && match end_order {
                        std::cmp::Ordering::Less => true,
                        std::cmp::Ordering::Greater => false,
                        std::cmp::Ordering::Equal => !*later_inclusive || *earlier_inclusive,
                    }
            }
            (
                MatchTest::Length {
                    count: later_count,
                    at_least: later_at_least,
                },
                MatchTest::Length {
                    count: earlier_count,
                    at_least: true,
                },
            ) => {
                let _ = later_at_least;
                later_count >= earlier_count
            }
            (
                MatchTest::Length {
                    count: later_count,
                    at_least: false,
                },
                MatchTest::Length {
                    count: earlier_count,
                    at_least: false,
                },
            ) => later_count == earlier_count,
            _ => false,
        }
    }

    fn scalar_in_range(
        &self,
        value: &MatchScalar,
        start: &MatchScalar,
        end: &MatchScalar,
        inclusive: bool,
    ) -> bool {
        let Some(lower) = self.match_scalar_cmp(value, start) else {
            return false;
        };
        let Some(upper) = self.match_scalar_cmp(value, end) else {
            return false;
        };
        lower != std::cmp::Ordering::Less
            && if inclusive {
                upper != std::cmp::Ordering::Greater
            } else {
                upper == std::cmp::Ordering::Less
            }
    }

    fn match_scalar_cmp(
        &self,
        left: &MatchScalar,
        right: &MatchScalar,
    ) -> Option<std::cmp::Ordering> {
        match (left, right) {
            (MatchScalar::Integer { text: left }, MatchScalar::Integer { text: right }) => {
                Some(parse_integer_value(left)?.cmp(&parse_integer_value(right)?))
            }
            (
                MatchScalar::Character { value: left },
                MatchScalar::Character { value: right },
            ) => Some(left.cmp(right)),
            (MatchScalar::String { value: left }, MatchScalar::String { value: right }) => {
                Some(left.cmp(right))
            }
            _ => None,
        }
    }

    fn match_condition_guaranteed_cases(
        &self,
        condition: &MatchCondition,
        required: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        match condition {
            MatchCondition::Always => required.clone(),
            MatchCondition::Never => BTreeSet::new(),
            MatchCondition::Test { projections, test } => {
                if !projections.is_empty() {
                    return BTreeSet::new();
                }
                let case = match test {
                    MatchTest::Bool { value } => Some(value.to_string()),
                    MatchTest::OptionNone => Some("None".to_owned()),
                    MatchTest::OptionSome => Some("Some".to_owned()),
                    MatchTest::Variant { name } => Some(name.clone()),
                    MatchTest::ScalarLiteral { .. }
                    | MatchTest::ScalarRange { .. }
                    | MatchTest::Length { .. } => None,
                };
                case.into_iter()
                    .filter(|case| required.contains(case))
                    .collect()
            }
            MatchCondition::All { conditions } => {
                let mut covered = required.clone();
                for condition in conditions {
                    let child = self.match_condition_guaranteed_cases(condition, required);
                    covered = covered.intersection(&child).cloned().collect();
                }
                covered
            }
            MatchCondition::Any { conditions } => conditions
                .iter()
                .flat_map(|condition| {
                    self.match_condition_guaranteed_cases(condition, required)
                })
                .collect(),
        }
    }

'''
text = text[:pos] + helpers + text[pos:]
tc.write_text(text)

# Step 7 has its own focused semantic tests, separate from FIR structural tests.
tests = Path("crates/forge-frontend/tests/typecheck.rs")
text = tests.read_text()
text += r'''

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
'''
tests.write_text(text)

plan = Path("docs/fir-completion-plan.md")
text = plan.read_text().replace(
    "- Steps 7-16 intentionally untouched.",
    "- Step 7 complete: exhaustiveness and unreachable-arm usefulness for planned patterns now consume the same semantic alternatives/conditions that FIR lowers.\n- Steps 8-16 intentionally untouched.",
    1,
)
plan.write_text(text)

Path("scripts/fir_step7_once.py").unlink()
Path(".github/workflows/fir-step7-once.yml").unlink()
