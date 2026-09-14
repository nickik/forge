from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing replacement anchor: {label}")
    if text.count(old) != 1:
        raise SystemExit(f"replacement anchor not unique ({text.count(old)}): {label}")
    return text.replace(old, new, 1)


typecheck_path = Path("crates/forge-frontend/src/typecheck_v1.rs")
t = typecheck_path.read_text()

# The semantic call plan is complete and parameter ordered before FIR.
t = replace_once(
    t,
    "#[derive(Debug, Clone, PartialEq, Serialize)]\n#[serde(tag = \"expr\", rename_all = \"snake_case\")]\npub enum TypedExprKind {",
    "#[derive(Debug, Clone, PartialEq, Serialize)]\n#[serde(tag = \"argument\", rename_all = \"snake_case\")]\npub enum ResolvedCallArgument {\n    Explicit {\n        argument: usize,\n    },\n    Default {\n        parameter: LocalId,\n        value: HirExpr,\n    },\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\n#[serde(tag = \"expr\", rename_all = \"snake_case\")]\npub enum TypedExprKind {",
    "resolved call argument enum",
)
t = t.replace("argument_parameters: Vec<usize>", "arguments: Vec<ResolvedCallArgument>")
t = replace_once(
    t,
    "struct ParamSig {\n    name: String,\n    ty: Ty,\n    has_default: bool,\n}",
    "struct ParamSig {\n    name: String,\n    ty: Ty,\n    has_default: bool,\n    default: Option<(LocalId, HirExpr)>,\n}",
    "param sig default",
)
t = replace_once(
    t,
    "let env = ModuleTypeEnv::build(source, module, &constant_values);",
    "let env = ModuleTypeEnv::build(source, module, &constant_values, bodies);",
    "env build call",
)
t = replace_once(
    t,
    "        constants: &BTreeMap<DefId, ConstValue>,\n    ) -> Self {",
    "        constants: &BTreeMap<DefId, ConstValue>,\n        bodies: &BodyHirOutput,\n    ) -> Self {",
    "env build signature",
)
# Both free functions and methods start with no attached HIR default; attach from BodyHir below.
t = t.replace(
    "                        has_default: p.default.is_some(),\n                    })",
    "                        has_default: p.default.is_some(),\n                        default: None,\n                    })",
)
if t.count("default: None,") != 2:
    raise SystemExit("expected direct-function and method ParamSig constructors")

t = replace_once(
    t,
    "            }\n        }\n        env\n    }\n\n    fn is_distinct_type",
    "            }\n        }\n\n        // Defaults are owned by the callee body.  Keep their resolved HIR and\n        // source parameter LocalId in the signature so call checking can build a\n        // complete parameter-order semantic plan without cloning/re-resolving the\n        // expression in the caller.\n        for (owner, body) in &bodies.functions {\n            let Some(sig) = env.functions.get_mut(owner) else {\n                continue;\n            };\n            for (param, (local, _)) in sig.params.iter_mut().zip(&body.params) {\n                param.default = body\n                    .param_defaults\n                    .get(local)\n                    .cloned()\n                    .map(|value| (*local, value));\n                param.has_default = param.default.is_some();\n            }\n        }\n        env\n    }\n\n    fn is_distinct_type",
    "attach defaults",
)

t = t.replace("argument_parameters", "arguments")

# Replace the argument checker with normalized final-parameter-order output.
start = t.index("    fn check_function_args(\n")
end = t.index("\n    fn check_type_call(", start)
new_check = '''    fn check_function_args(
        &mut self,
        span: Span,
        sig: &FunctionSig,
        args: &[HirCallArg],
    ) -> Vec<ResolvedCallArgument> {
        let named = args.iter().any(|a| matches!(a, HirCallArg::Named { .. }));
        let mut slots = vec![None; sig.params.len()];
        if named {
            if !sig.named_arguments {
                self.diagnostic(
                    span,
                    "call/unknown-name",
                    "named arguments require an nfn declaration",
                );
                return Vec::new();
            }
            let mut seen = BTreeSet::new();
            for (argument, arg) in args.iter().enumerate() {
                let HirCallArg::Named { name, value } = arg else {
                    self.diagnostic(
                        span,
                        "call/mixed-arguments",
                        "named calls cannot mix positional and named arguments",
                    );
                    continue;
                };
                if !seen.insert(name.clone()) {
                    self.diagnostic(
                        value.span,
                        "call/duplicate-name",
                        format!("duplicate named argument `{name}`"),
                    );
                    continue;
                }
                if let Some((parameter, param)) =
                    sig.params.iter().enumerate().find(|(_, p)| p.name == *name)
                {
                    slots[parameter] = Some(ResolvedCallArgument::Explicit { argument });
                    let actual = self.check_expr(value, Some(&param.ty));
                    self.require_assignable(value.span, &param.ty, &actual, "type/mismatch");
                } else {
                    self.diagnostic(
                        value.span,
                        "call/unknown-name",
                        format!("unknown named argument `{name}`"),
                    );
                }
            }
        } else {
            if sig.named_arguments && !args.is_empty() {
                self.diagnostic(
                    span,
                    "call/named-only",
                    "nfn calls require named arguments",
                );
            }
            for (argument, arg) in args.iter().enumerate() {
                let value = arg_value(arg);
                if let Some(param) = sig.params.get(argument) {
                    slots[argument] = Some(ResolvedCallArgument::Explicit { argument });
                    let actual = self.check_expr(value, Some(&param.ty));
                    self.require_assignable(value.span, &param.ty, &actual, "type/mismatch");
                } else {
                    self.diagnostic(value.span, "call/arity", "too many arguments");
                }
            }
        }

        for (parameter, param) in sig.params.iter().enumerate() {
            if slots[parameter].is_some() {
                continue;
            }
            if let Some((local, value)) = &param.default {
                slots[parameter] = Some(ResolvedCallArgument::Default {
                    parameter: *local,
                    value: value.clone(),
                });
            } else {
                self.diagnostic(
                    span,
                    "call/missing-argument",
                    format!("missing required argument `{}`", param.name),
                );
            }
        }
        slots.into_iter().flatten().collect()
    }
'''
t = t[:start] + new_check + t[end:]

typecheck_path.write_text(t)

# Publicly expose the semantic call-plan node.
lib_path = Path("crates/forge-frontend/src/lib.rs")
l = lib_path.read_text()
l = replace_once(
    l,
    "    MatchTest, ResolvedReceiver, Ty, TypeCheckOutput, TypeDiagnostic, TypedBody, TypedExpr,\n",
    "    MatchTest, ResolvedCallArgument, ResolvedReceiver, Ty, TypeCheckOutput, TypeDiagnostic, TypedBody, TypedExpr,\n",
    "lib call argument export",
)
lib_path.write_text(l)

# FIR consumes only the normalized plan.  Callee defaults temporarily switch the
# expression/local semantic context to the callee while emitting into the caller.
fir_path = Path("crates/forge-frontend/src/fir_v1.rs")
f = fir_path.read_text()
f = replace_once(
    f,
    "        ResolvedReceiver, Ty, TypeCheckOutput, TypedBody, TypedExpr, TypedExprKind,\n",
    "        ResolvedCallArgument, ResolvedReceiver, Ty, TypeCheckOutput, TypedBody, TypedExpr, TypedExprKind,\n",
    "fir import",
)
f = f.replace("argument_parameters", "arguments")
f = replace_once(
    f,
    "    local_map: BTreeMap<LocalId, FirLocalId>,\n    overflow: OverflowMode,",
    "    local_map: BTreeMap<LocalId, FirLocalId>,\n    local_constants: BTreeMap<LocalId, ConstValue>,\n    overflow: OverflowMode,",
    "lowerer local constants field",
)
f = replace_once(
    f,
    "            exprs,\n            local_map,\n            overflow,",
    "            exprs,\n            local_map,\n            local_constants: typed.local_constants.clone(),\n            overflow,",
    "lowerer local constants init",
)
f = f.replace("self.typed.local_constants.get(&local).cloned()", "self.local_constants.get(&local).cloned()")

start = f.index("    #[allow(clippy::too_many_arguments)]\n    fn lower_resolved_call(\n")
end = f.index("\n    fn lower_try(\n", start)
new_lower = '''    #[allow(clippy::too_many_arguments)]
    fn lower_resolved_call(
        &mut self,
        expr: &HirExpr,
        target: DefId,
        method: bool,
        receiver: Option<ResolvedReceiver>,
        arguments: &[ResolvedCallArgument],
        result_ty: Ty,
        tail: bool,
    ) -> FirValueId {
        let HirExprKind::Call { callee, args } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/call-shape",
                "resolved call is not a call HIR node",
            );
            return self.poison(expr.span, result_ty);
        };
        let Some(target_body) = self.all_typed.functions.get(&target) else {
            self.diagnostic(
                expr.span,
                "fir/call-target-signature",
                format!("missing typed signature for call target {target:?}"),
            );
            return self.poison(expr.span, result_ty);
        };
        let target_params = target_body.params.clone();
        let mut placed: Vec<Option<FirValueId>> = vec![None; target_params.len()];
        let mut parameter_locals = BTreeMap::new();
        let offset = if method { 1 } else { 0 };

        if method {
            let HirExprKind::Member { base, .. } = &callee.kind else {
                self.diagnostic(
                    expr.span,
                    "fir/method-shape",
                    "resolved method call has no member receiver",
                );
                return self.poison(expr.span, result_ty);
            };
            let receiver_value = match receiver.unwrap_or(ResolvedReceiver::Value) {
                ResolvedReceiver::Value => self.lower_expr(base),
                ResolvedReceiver::SharedReference | ResolvedReceiver::MutableReference => {
                    let expected_ref = target_params
                        .first()
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(Ty::Error);
                    let base_ty = self.expr_ty(base);
                    if matches!(base_ty, Ty::Reference { .. }) {
                        self.lower_expr(base)
                    } else if let Some(place) = self.lower_place(base) {
                        self.emit_value(
                            base.span,
                            expected_ref,
                            FirInstructionKind::AddressOf {
                                place,
                                mutable: matches!(
                                    receiver,
                                    Some(ResolvedReceiver::MutableReference)
                                ),
                            },
                        )
                    } else {
                        self.diagnostic(
                            base.span,
                            "fir/method-receiver-place",
                            "reference receiver was resolved but the receiver is not a lowerable place",
                        );
                        self.poison(base.span, expected_ref)
                    }
                }
            };
            if let Some((source, ty)) = target_params.first() {
                let local = self.synthetic_local(ty.clone());
                self.emit_void(
                    base.span,
                    FirInstructionKind::Store {
                        place: FirPlace::Local { local },
                        value: receiver_value,
                    },
                );
                parameter_locals.insert(*source, local);
                placed[0] = Some(receiver_value);
            }
        }

        if arguments.len() + offset != target_params.len() {
            self.diagnostic(
                expr.span,
                "fir/call-plan",
                "resolved call plan does not contain exactly one entry per non-receiver parameter",
            );
        }

        for (slot, argument) in arguments.iter().enumerate() {
            let parameter = slot + offset;
            let Some((source_local, parameter_ty)) = target_params.get(parameter).cloned() else {
                self.diagnostic(expr.span, "fir/call-plan", "call plan references an invalid parameter");
                continue;
            };
            let value = match argument {
                ResolvedCallArgument::Explicit { argument } => {
                    let Some(arg) = args.get(*argument) else {
                        self.diagnostic(expr.span, "fir/call-plan", "call plan references a missing explicit argument");
                        continue;
                    };
                    self.lower_expr(arg_value(arg))
                }
                ResolvedCallArgument::Default { parameter, value } => {
                    if *parameter != source_local {
                        self.diagnostic(
                            expr.span,
                            "fir/call-plan",
                            "default argument is attached to the wrong target parameter",
                        );
                    }
                    self.lower_default_argument(target, value, &parameter_locals)
                }
            };
            let local = self.synthetic_local(parameter_ty);
            self.emit_void(
                expr.span,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    value,
                },
            );
            parameter_locals.insert(source_local, local);
            placed[parameter] = Some(value);
        }

        if placed.iter().any(Option::is_none) {
            self.diagnostic(
                expr.span,
                "fir/call-plan-incomplete",
                "typed HIR call plan left a target parameter without a value",
            );
            for (index, slot) in placed.iter_mut().enumerate() {
                if slot.is_none() {
                    let ty = target_params
                        .get(index)
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(Ty::Error);
                    *slot = Some(self.poison(expr.span, ty));
                }
            }
        }
        let args = placed.into_iter().flatten().collect();
        self.emit_value(
            expr.span,
            result_ty,
            FirInstructionKind::Call { target, args, tail },
        )
    }

    fn lower_default_argument(
        &mut self,
        target: DefId,
        value: &HirExpr,
        parameter_locals: &BTreeMap<LocalId, FirLocalId>,
    ) -> FirValueId {
        let all_typed = self.all_typed;
        let Some(target_body) = all_typed.functions.get(&target) else {
            self.diagnostic(
                value.span,
                "fir/call-target-signature",
                format!("missing typed body for default argument target {target:?}"),
            );
            return self.poison(value.span, Ty::Error);
        };
        let target_exprs = target_body
            .expressions
            .iter()
            .map(|expr| (expr.id, expr))
            .collect();
        let saved_exprs = std::mem::replace(&mut self.exprs, target_exprs);
        let saved_locals = std::mem::replace(&mut self.local_map, parameter_locals.clone());
        let saved_constants = std::mem::replace(
            &mut self.local_constants,
            target_body.local_constants.clone(),
        );
        let result = self.lower_expr(value);
        self.exprs = saved_exprs;
        self.local_map = saved_locals;
        self.local_constants = saved_constants;
        result
    }
'''
f = f[:start] + new_lower + f[end:]
fir_path.write_text(f)

# Typechecker-focused plan test.
test_path = Path("crates/forge-frontend/tests/typecheck.rs")
test = test_path.read_text()
test += r'''

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
                if arguments.len() == 3 => Some(arguments),
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
'''
test_path.write_text(test)

# FIR tests: complete defaults, earlier-parameter references, and evaluation once.
fir_test_path = Path("crates/forge-frontend/tests/fir.rs")
ft = fir_test_path.read_text()
ft += r'''

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
    assert!(instructions(&output).filter(|op| matches!(
        op,
        FirInstructionKind::Binary { op: forge_frontend::ast::BinaryOp::Add, .. }
    )).count() >= 2);
}
'''
fir_test_path.write_text(ft)

# Completion record.
plan_path = Path("docs/fir-completion-plan.md")
p = plan_path.read_text()
p = p.replace(
    "- Steps 8-16 intentionally untouched.",
    "- Step 8 complete: direct/named calls carry complete parameter-order argument plans; omitted defaults remain callee-owned typed HIR and FIR evaluates them in parameter order with earlier parameter values materialized exactly once.\n- Steps 9-16 intentionally untouched.",
)
p += """

## Step 8 acceptance tests

- Typed HIR contains exactly one normalized call-plan entry per target parameter (excluding a separately normalized method receiver).
- Named explicit arguments are mapped to final parameter order independent of source order.
- Omitted defaults are represented explicitly in typed HIR and never rediscovered by FIR.
- A default may reference an earlier parameter; FIR materializes earlier parameter values into synthetic locals before lowering that default.
- Chained defaults therefore observe the already-evaluated earlier parameter/default value without re-evaluating an explicit source argument.
- `fir/default-argument-not-materialized` is removed; an incomplete semantic plan is instead an internal `fir/call-plan-incomplete` boundary failure.
"""
plan_path.write_text(p)
