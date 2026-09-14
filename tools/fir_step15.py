from pathlib import Path


def replace(path, old, new):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:80]!r}")
    p.write_text(text.replace(old, new, 1))

# body_hir: preserve locals created while lowering global initializer expressions.
replace(
    "crates/forge-frontend/src/body_hir_v1.rs",
    "pub struct HirGlobalBody {\n    pub owner: DefId,\n    pub ty: Option<HirType>,\n    pub value: HirExpr,\n}",
    "pub struct HirGlobalBody {\n    pub owner: DefId,\n    pub ty: Option<HirType>,\n    pub locals: Vec<HirLocalDecl>,\n    pub value: HirExpr,\n}",
)
replace(
    "crates/forge-frontend/src/body_hir_v1.rs",
    "                let ty = value.ty.as_ref().map(|ty| lowerer.lower_type(ty));\n                let expr = lowerer.lower_expr(&value.value);\n                output.globals.insert(\n                    owner,\n                    HirGlobalBody {\n                        owner,\n                        ty,\n                        value: expr,\n                    },\n                );",
    "                let ty = value.ty.as_ref().map(|ty| lowerer.lower_type(ty));\n                let expr = lowerer.lower_expr(&value.value);\n                let locals = lowerer.locals;\n                output.globals.insert(\n                    owner,\n                    HirGlobalBody {\n                        owner,\n                        ty,\n                        locals,\n                        value: expr,\n                    },\n                );",
)

# typecheck: retain typed runtime initializer bodies + direct runtime dependencies.
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    "#[derive(Debug, Clone, PartialEq, Serialize, Default)]\npub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub global_types: BTreeMap<DefId, Ty>,",
    "#[derive(Debug, Clone, PartialEq, Serialize)]\npub struct TypedGlobalInitializer {\n    pub owner: DefId,\n    pub span: Span,\n    pub ty: Ty,\n    pub dependencies: Vec<DefId>,\n    pub body: TypedBody,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize, Default)]\npub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub global_types: BTreeMap<DefId, Ty>,\n    pub global_initializers: BTreeMap<DefId, TypedGlobalInitializer>,\n    pub global_init_order: Vec<DefId>,",
)
old_global = '''    for (owner, global) in &bodies.globals {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let expected = global
            .ty
            .as_ref()
            .map(|annotation| env.lower_hir_type(annotation));
        let value_ty = checker.check_expr(&global.value, expected.as_ref());
        let ty = if let Some(expected) = expected {
            checker.require_assignable(global.value.span, &expected, &value_ty, "type/mismatch");
            expected
        } else {
            checker.materialize_literal(global.value.span, value_ty)
        };
        output.global_types.insert(*owner, ty);
    }

    output
}
'''
new_global = '''    for (owner, global) in &bodies.globals {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let expected = global
            .ty
            .as_ref()
            .map(|annotation| env.lower_hir_type(annotation));
        let value_ty = checker.check_expr(&global.value, expected.as_ref());
        let ty = if let Some(expected) = expected {
            checker.require_assignable(global.value.span, &expected, &value_ty, "type/mismatch");
            expected
        } else {
            checker.materialize_literal(global.value.span, value_ty)
        };
        output.global_types.insert(*owner, ty.clone());

        let is_runtime = source
            .declarations
            .get(owner.0 as usize)
            .is_some_and(|declaration| {
                matches!(
                    &declaration.kind.kind,
                    DeclKind::Global(value) if !matches!(value.binding, ast::BindingKind::Const)
                )
            });
        if !is_runtime {
            continue;
        }

        let dependencies = checker
            .expressions
            .iter()
            .filter_map(|typed| typed_expr_hir(&typed.kind))
            .filter_map(|hir| match &hir.kind {
                HirExprKind::Name { reference } => match reference.root {
                    ResolvedName::Def(id) if bodies.globals.contains_key(&id) => Some(id),
                    _ => None,
                },
                _ => None,
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let body = TypedBody {
            owner: *owner,
            params: Vec::new(),
            return_type: ty.clone(),
            local_types: checker.local_types,
            local_constants: checker.local_constants,
            context_scopes: checker.context_scopes,
            select_plans: checker.select_plans,
            unsafe_scopes: checker.unsafe_scopes,
            expressions: checker.expressions,
        };
        output.global_initializers.insert(
            *owner,
            TypedGlobalInitializer {
                owner: *owner,
                span: global.value.span,
                ty,
                dependencies,
                body,
            },
        );
    }

    let runtime_ids = output
        .global_initializers
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    for initializer in output.global_initializers.values_mut() {
        initializer
            .dependencies
            .retain(|dependency| runtime_ids.contains(dependency));
    }
    output.global_init_order = order_global_initializers(
        &output.global_initializers,
        &mut output.diagnostics,
    );

    output
}

fn typed_expr_hir(kind: &TypedExprKind) -> Option<&HirExpr> {
    match kind {
        TypedExprKind::Source { hir }
        | TypedExprKind::ResolvedCall { hir, .. }
        | TypedExprKind::ResolvedClosure { hir, .. }
        | TypedExprKind::ResolvedContext { hir, .. }
        | TypedExprKind::ResolvedTry { hir, .. }
        | TypedExprKind::ResolvedMatch { hir, .. }
        | TypedExprKind::UnsafeOperation { hir, .. }
        | TypedExprKind::ResolvedBitField { hir, .. }
        | TypedExprKind::OptionalPromote { hir, .. } => Some(hir),
    }
}

fn order_global_initializers(
    initializers: &BTreeMap<DefId, TypedGlobalInitializer>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> Vec<DefId> {
    let mut indegree = initializers
        .keys()
        .copied()
        .map(|owner| (owner, 0usize))
        .collect::<BTreeMap<_, _>>();
    let mut dependents = BTreeMap::<DefId, Vec<DefId>>::new();
    for (owner, initializer) in initializers {
        for dependency in &initializer.dependencies {
            if !initializers.contains_key(dependency) {
                continue;
            }
            *indegree.get_mut(owner).expect("initializer owner") += 1;
            dependents.entry(*dependency).or_default().push(*owner);
        }
    }
    for values in dependents.values_mut() {
        values.sort();
        values.dedup();
    }

    let mut ready = indegree
        .iter()
        .filter_map(|(owner, degree)| (*degree == 0).then_some(*owner))
        .collect::<BTreeSet<_>>();
    let mut order = Vec::with_capacity(initializers.len());
    while let Some(owner) = ready.pop_first() {
        order.push(owner);
        if let Some(users) = dependents.get(&owner) {
            for user in users {
                let degree = indegree.get_mut(user).expect("dependent initializer");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*user);
                }
            }
        }
    }

    if order.len() != initializers.len() {
        let ordered = order.iter().copied().collect::<BTreeSet<_>>();
        let cyclic = initializers
            .keys()
            .copied()
            .filter(|owner| !ordered.contains(owner))
            .collect::<Vec<_>>();
        for owner in &cyclic {
            if let Some(initializer) = initializers.get(owner) {
                diagnostics.push(TypeDiagnostic {
                    span: initializer.span,
                    code: "global/init-cycle".into(),
                    message: format!(
                        "runtime global initializer {:?} participates in a dependency cycle",
                        owner
                    ),
                });
            }
        }
        // Keep invalid semantic output deterministic for diagnostics/debug dumps.
        order.extend(cyclic);
    }
    order
}
'''
replace("crates/forge-frontend/src/typecheck_v1.rs", old_global, new_global)

# FIR module: explicit initializer functions + dependency order.
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    "        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirMatchBody,\n        HirPattern, HirPatternKind, HirStmt, HirStmtKind,",
    "        BodyHirOutput, ExprId, HirBlock, HirBody, HirCallArg, HirExpr, HirExprKind, HirMatchBody,\n        HirPattern, HirPatternKind, HirStmt, HirStmtKind,",
)
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    "pub struct FirModule {\n    pub functions: BTreeMap<DefId, FirFunction>,\n    pub globals: BTreeMap<DefId, FirGlobal>,\n}",
    "pub struct FirModule {\n    pub functions: BTreeMap<DefId, FirFunction>,\n    pub globals: BTreeMap<DefId, FirGlobal>,\n    pub global_initializers: BTreeMap<DefId, FirGlobalInitializer>,\n    pub global_init_order: Vec<DefId>,\n}",
)
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    "pub struct FirGlobal {\n    pub owner: DefId,\n    pub ty: Ty,\n    pub constant: Option<ConstValue>,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\npub struct FirFunction",
    "pub struct FirGlobal {\n    pub owner: DefId,\n    pub ty: Ty,\n    pub constant: Option<ConstValue>,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\npub struct FirGlobalInitializer {\n    pub owner: DefId,\n    pub dependencies: Vec<DefId>,\n    pub function: FirFunction,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\npub struct FirFunction",
)
marker = '''    for (owner, body) in &bodies.functions {
'''
insert = '''    output.module.global_init_order = typed.global_init_order.clone();
    for owner in &typed.global_init_order {
        let Some(plan) = typed.global_initializers.get(owner) else {
            output.diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/global-init-plan".into(),
                message: format!("missing typed runtime global initializer for {owner:?}"),
            });
            continue;
        };
        let Some(global) = bodies.globals.get(owner) else {
            output.diagnostics.push(FirDiagnostic {
                span: plan.span,
                code: "fir/global-init-body".into(),
                message: format!("missing HIR runtime global initializer for {owner:?}"),
            });
            continue;
        };
        let synthetic = HirBody {
            owner: *owner,
            params: Vec::new(),
            param_defaults: BTreeMap::new(),
            return_type: global.ty.clone(),
            locals: global.locals.clone(),
            block: HirBlock {
                span: global.value.span,
                statements: vec![HirStmt {
                    span: global.value.span,
                    kind: HirStmtKind::Return {
                        tail: false,
                        value: Some(global.value.clone()),
                    },
                }],
            },
        };
        let (function, mut diagnostics) = FunctionLowerer::new(
            &synthetic,
            &plan.body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        diagnostics.extend(verify_fir_function(&function));
        output.diagnostics.extend(diagnostics);
        output.module.global_initializers.insert(
            *owner,
            FirGlobalInitializer {
                owner: *owner,
                dependencies: plan.dependencies.clone(),
                function,
            },
        );
    }

    for (owner, body) in &bodies.functions {
'''
replace("crates/forge-frontend/src/fir_v1.rs", marker, insert)

# Public exports.
replace(
    "crates/forge-frontend/src/lib.rs",
    "    TypedExprKind, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope,\n    UnsafeOperationKind, UnsafeProvenance,",
    "    TypedExprKind, TypedGlobalInitializer, TypedMatchArmPlan, TypedMatchBinding, TypedMatchPlan,\n    TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,",
)
replace(
    "crates/forge-frontend/src/lib.rs",
    "    FirFunction, FirGlobal, FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule,",
    "    FirFunction, FirGlobal, FirGlobalInitializer, FirInstruction, FirInstructionKind, FirLocal,\n    FirLocalId, FirModule,",
)

# Focused semantic tests.
p = Path("crates/forge-frontend/tests/typecheck.rs")
text = p.read_text()
text += r'''

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
    assert_eq!(output.global_initializers[&second].dependencies, vec![first]);
    assert!(first.0 > second.0, "dependency declared later must initialize first");
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
    assert!(has(&output, "global/init-cycle"), "{:?}", output.diagnostics);
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
'''
p.write_text(text)

p = Path("crates/forge-frontend/tests/fir.rs")
text = p.read_text()
text += r'''

#[test]
fn runtime_globals_lower_to_explicit_initializer_functions() {
    let output = lower(
        r#"
        module test.fir_global_init;
        fn seed() -> u32 { return 4u32; }
        val dependent: u32 = base + 1u32;
        val base: u32 = seed();
        const fixed: u32 = 9u32;
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.module.global_initializers.len(), 2);
    assert_eq!(output.module.global_init_order.len(), 2);
    assert_eq!(
        output
            .module
            .globals
            .values()
            .filter(|global| global.constant.is_some())
            .count(),
        1
    );

    let base = output.module.global_init_order[0];
    let dependent = output.module.global_init_order[1];
    assert!(output.module.global_initializers[&base].dependencies.is_empty());
    assert_eq!(
        output.module.global_initializers[&dependent].dependencies,
        vec![base]
    );

    let base_calls = output.module.global_initializers[&base]
        .function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| matches!(instruction.kind, FirInstructionKind::Call { .. }))
        .count();
    assert_eq!(base_calls, 1, "runtime initializer expression must execute once");
    assert!(output.module.global_initializers[&dependent]
        .function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .any(|instruction| matches!(
            instruction.kind,
            FirInstructionKind::LoadGlobal { global } if global == base
        )));
}
'''
p.write_text(text)

# Plan completion record + acceptance tests.
p = Path("docs/fir-completion-plan.md")
text = p.read_text()
text = text.replace(
    "- Steps 15-16 intentionally untouched.",
    "- Step 15 complete: compile-time `const` globals remain static data; `val`/`var` globals carry typed runtime initializer bodies, direct runtime-global dependencies, deterministic dependency-first/source-order initialization, and explicit FIR initializer functions.\n- Step 16 intentionally untouched.",
)
text += r'''

## Step 15 acceptance tests

- Compile-time `const` globals remain static FIR data and do not receive runtime initializer functions.
- Every non-`const` global receives a typed runtime initializer body whose result type is the finalized global type.
- Direct references from one runtime global initializer to another are recorded as semantic dependencies above FIR; references to compile-time constants do not create runtime dependencies.
- Runtime initializers are topologically ordered so dependencies execute first; independent globals retain declaration/source order through stable `DefId` ordering.
- Runtime dependency cycles are rejected semantically with `global/init-cycle` rather than left to backend/linker behavior.
- FIR represents each runtime initializer as an explicit zero-argument initializer function returning the global value and publishes the deterministic module `global_init_order`.
- FIR consumes the typed dependency plan without rediscovering global references or choosing an initialization order.
- Each initializer source expression is lowered exactly once; ordinary expression evaluation order remains unchanged inside the initializer function.
'''
p.write_text(text)
