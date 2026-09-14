from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement target in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


# Preserve const-ness in body HIR instead of collapsing const to val.
path = Path("crates/forge-frontend/src/body_hir_v1.rs")
text = path.read_text()
old = '''    Value {
        mutable: bool,
        pattern: HirPattern,
        ty: Option<HirType>,
        value: HirExpr,
    },
'''
new = '''    Value {
        mutable: bool,
        constant: bool,
        pattern: HirPattern,
        ty: Option<HirType>,
        value: HirExpr,
    },
'''
if old not in text:
    raise SystemExit("HirStmtKind::Value definition missing")
text = text.replace(old, new, 1)

old = '''                let mutable = matches!(value.binding, ast::BindingKind::Var);
                let pattern = self.lower_binding_pattern(&value.pattern, mutable);
                HirStmtKind::Value {
                    mutable,
                    pattern,
                    ty,
                    value: expr,
                }
'''
new = '''                let mutable = matches!(value.binding, ast::BindingKind::Var);
                let constant = matches!(value.binding, ast::BindingKind::Const);
                let pattern = self.lower_binding_pattern(&value.pattern, mutable);
                HirStmtKind::Value {
                    mutable,
                    constant,
                    pattern,
                    ty,
                    value: expr,
                }
'''
if old not in text:
    raise SystemExit("ordinary value lowering block missing")
text = text.replace(old, new, 1)

old = '''                let mutable = matches!(value.binding, ast::BindingKind::Var);
                let pattern = self.lower_binding_pattern(&value.pattern, mutable);
                HirNode::new(
                    span,
                    HirStmtKind::Value {
                        mutable,
                        pattern,
                        ty,
                        value: expr,
                    },
                )
'''
new = '''                let mutable = matches!(value.binding, ast::BindingKind::Var);
                let constant = matches!(value.binding, ast::BindingKind::Const);
                let pattern = self.lower_binding_pattern(&value.pattern, mutable);
                HirNode::new(
                    span,
                    HirStmtKind::Value {
                        mutable,
                        constant,
                        pattern,
                        ty,
                        value: expr,
                    },
                )
'''
if old not in text:
    raise SystemExit("for-init value lowering block missing")
text = text.replace(old, new, 1)
path.write_text(text)


# Type checker: retain/evaluate local constants and allow them in local array lengths.
path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()
old = '''pub struct TypedBody {
    pub owner: DefId,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub expressions: Vec<TypedExpr>,
}
'''
new = '''pub struct TypedBody {
    pub owner: DefId,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
}
'''
if old not in text:
    raise SystemExit("TypedBody block missing")
text = text.replace(old, new, 1)

old = '''            TypedBody {
                owner: *owner,
                local_types: checker.local_types,
                expressions: checker.expressions,
            },
'''
new = '''            TypedBody {
                owner: *owner,
                local_types: checker.local_types,
                local_constants: checker.local_constants,
                expressions: checker.expressions,
            },
'''
if old not in text:
    raise SystemExit("TypedBody construction missing")
text = text.replace(old, new, 1)

# Replace lower_hir_type with a local-constant-aware variant.
start = text.find("    fn lower_hir_type(&self, ty: &HirType) -> Ty {")
end = text.find("    fn ty_from_ref(&self, reference: &HirTypeRef) -> Ty {", start)
if start < 0 or end < 0:
    raise SystemExit("lower_hir_type block markers missing")
new_lower = '''    fn lower_hir_type(&self, ty: &HirType) -> Ty {
        self.lower_hir_type_with_locals(ty, &BTreeMap::new())
    }

    fn lower_hir_type_with_locals(
        &self,
        ty: &HirType,
        local_constants: &BTreeMap<LocalId, ConstValue>,
    ) -> Ty {
        match &ty.kind {
            HirTypeKind::Named { reference } => self.ty_from_ref(reference),
            HirTypeKind::Pointer { volatile, inner } => Ty::Pointer {
                volatile: *volatile,
                inner: Box::new(self.lower_hir_type_with_locals(inner, local_constants)),
            },
            HirTypeKind::Reference { mutable, inner } => Ty::Reference {
                mutable: *mutable,
                inner: Box::new(self.lower_hir_type_with_locals(inner, local_constants)),
            },
            HirTypeKind::Optional { inner } => Ty::Optional {
                inner: Box::new(self.lower_hir_type_with_locals(inner, local_constants)),
            },
            HirTypeKind::Slice { mutable, element } => Ty::Slice {
                mutable: *mutable,
                element: Box::new(self.lower_hir_type_with_locals(element, local_constants)),
            },
            HirTypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_hir_type_with_locals(element, local_constants)),
                length: eval_const_hir_with_locals(length, &self.constants, local_constants)
                    .ok()
                    .and_then(const_value_to_u64),
            },
            HirTypeKind::Result { ok, error } => Ty::Result {
                ok: Box::new(self.lower_hir_type_with_locals(ok, local_constants)),
                error: Box::new(self.lower_hir_type_with_locals(error, local_constants)),
            },
            HirTypeKind::Function { params, result } => Ty::Function {
                params: params
                    .iter()
                    .map(|param| self.lower_hir_type_with_locals(param, local_constants))
                    .collect(),
                result: Box::new(self.lower_hir_type_with_locals(result, local_constants)),
                named_arguments: false,
            },
            HirTypeKind::Closure { params, result } => Ty::Closure {
                params: params
                    .iter()
                    .map(|param| self.lower_hir_type_with_locals(param, local_constants))
                    .collect(),
                result: Box::new(self.lower_hir_type_with_locals(result, local_constants)),
            },
        }
    }

'''
text = text[:start] + new_lower + text[end:]

old = '''struct BodyChecker<'a, 'd> {
    env: &'a ModuleTypeEnv,
    expected_return: Ty,
    local_types: BTreeMap<LocalId, Ty>,
    mutable_locals: BTreeSet<LocalId>,
    expressions: Vec<TypedExpr>,
    diagnostics: &'d mut Vec<TypeDiagnostic>,
}
'''
new = '''struct BodyChecker<'a, 'd> {
    env: &'a ModuleTypeEnv,
    expected_return: Ty,
    local_types: BTreeMap<LocalId, Ty>,
    local_constants: BTreeMap<LocalId, ConstValue>,
    mutable_locals: BTreeSet<LocalId>,
    expressions: Vec<TypedExpr>,
    diagnostics: &'d mut Vec<TypeDiagnostic>,
}
'''
if old not in text:
    raise SystemExit("BodyChecker struct missing")
text = text.replace(old, new, 1)

old = '''            expected_return,
            local_types: BTreeMap::new(),
            mutable_locals: BTreeSet::new(),
'''
new = '''            expected_return,
            local_types: BTreeMap::new(),
            local_constants: BTreeMap::new(),
            mutable_locals: BTreeSet::new(),
'''
if old not in text:
    raise SystemExit("BodyChecker constructor missing")
text = text.replace(old, new, 1)

old = '''            HirStmtKind::Value {
                mutable,
                pattern,
                ty,
                value,
            } => {
                let expected = ty.as_ref().map(|t| self.env.lower_hir_type(t));
                let actual = self.check_expr(value, expected.as_ref());
                let final_ty = if let Some(expected) = expected {
                    self.require_assignable(value.span, &expected, &actual, "type/mismatch");
                    expected
                } else {
                    self.materialize_literal(value.span, actual)
                };
                self.check_irrefutable_binding_pattern(pattern, &final_ty);
                self.check_pattern(pattern, &final_ty);
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
            }
'''
new = '''            HirStmtKind::Value {
                mutable,
                constant,
                pattern,
                ty,
                value,
            } => {
                let expected = ty.as_ref().map(|t| {
                    self.env
                        .lower_hir_type_with_locals(t, &self.local_constants)
                });
                let actual = self.check_expr(value, expected.as_ref());
                let final_ty = if let Some(expected) = expected {
                    self.require_assignable(value.span, &expected, &actual, "type/mismatch");
                    expected
                } else {
                    self.materialize_literal(value.span, actual)
                };
                self.check_irrefutable_binding_pattern(pattern, &final_ty);
                self.check_pattern(pattern, &final_ty);
                if *constant {
                    match eval_const_hir_with_locals(
                        value,
                        &self.env.constants,
                        &self.local_constants,
                    ) {
                        Ok(const_value) => match &pattern.kind {
                            HirPatternKind::Binding { local, .. } => {
                                self.local_constants.insert(*local, const_value);
                            }
                            _ => self.diagnostic(
                                pattern.span,
                                "const/pattern",
                                "Forge v1 compile-time `const` bindings require a single name",
                            ),
                        },
                        Err(error) => self.diagnostic(
                            error.span,
                            "const/eval",
                            error.message,
                        ),
                    }
                }
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
            }
'''
if old not in text:
    raise SystemExit("BodyChecker value arm missing")
text = text.replace(old, new, 1)

# Closure type annotations may refer to an outer local const.
text = text.replace(
'''                        let ty = self.env.lower_hir_type(t);
                        self.local_types.insert(*id, ty.clone());
''',
'''                        let ty = self
                            .env
                            .lower_hir_type_with_locals(t, &self.local_constants);
                        self.local_types.insert(*id, ty.clone());
''',
1,
)
text = text.replace(
'''                let result = return_type
                    .as_ref()
                    .map(|t| self.env.lower_hir_type(t))
                    .unwrap_or(Ty::Unknown);
''',
'''                let result = return_type
                    .as_ref()
                    .map(|t| {
                        self.env
                            .lower_hir_type_with_locals(t, &self.local_constants)
                    })
                    .unwrap_or(Ty::Unknown);
''',
1,
)

# Make the resolved HIR constant evaluator accept local compile-time bindings too.
old = '''fn eval_const_hir_resolved(
    expr: &HirExpr,
    constants: &BTreeMap<DefId, ConstValue>,
) -> Result<ConstValue, ConstEvalError> {
    match &expr.kind {
'''
new = '''fn eval_const_hir_resolved(
    expr: &HirExpr,
    constants: &BTreeMap<DefId, ConstValue>,
) -> Result<ConstValue, ConstEvalError> {
    eval_const_hir_with_locals(expr, constants, &BTreeMap::new())
}

fn eval_const_hir_with_locals(
    expr: &HirExpr,
    constants: &BTreeMap<DefId, ConstValue>,
    local_constants: &BTreeMap<LocalId, ConstValue>,
) -> Result<ConstValue, ConstEvalError> {
    match &expr.kind {
'''
if old not in text:
    raise SystemExit("eval_const_hir_resolved start missing")
text = text.replace(old, new, 1)

old = '''        HirExprKind::Name { reference } if reference.tail.is_empty() => match reference.root {
            ResolvedName::Def(id) => constants
                .get(&id)
                .cloned()
                .ok_or_else(|| ConstEvalError {
                    span: expr.span,
                    message: format!("definition {id:?} is not an evaluable module `const`"),
                }),
            _ => Err(ConstEvalError {
                span: expr.span,
                message: "constant expression may reference only module `const` definitions".into(),
            }),
        },
'''
new = '''        HirExprKind::Name { reference } if reference.tail.is_empty() => match reference.root {
            ResolvedName::Def(id) => constants
                .get(&id)
                .cloned()
                .ok_or_else(|| ConstEvalError {
                    span: expr.span,
                    message: format!("definition {id:?} is not an evaluable module `const`"),
                }),
            ResolvedName::Local(id) => local_constants
                .get(&id)
                .cloned()
                .ok_or_else(|| ConstEvalError {
                    span: expr.span,
                    message: format!("local {id:?} is not a compile-time `const`"),
                }),
            _ => Err(ConstEvalError {
                span: expr.span,
                message: "constant expression may reference only compile-time `const` bindings"
                    .into(),
            }),
        },
'''
if old not in text:
    raise SystemExit("resolved HIR const name arm missing")
text = text.replace(old, new, 1)

# Recursive calls inside the local-aware evaluator must preserve local constants.
segment_start = text.find("fn eval_const_hir_with_locals(")
segment_end = text.find("fn apply_const_unary(", segment_start)
if segment_start < 0 or segment_end < 0:
    raise SystemExit("local evaluator segment markers missing")
segment = text[segment_start:segment_end]
segment = segment.replace(
    "eval_const_hir_resolved(value, constants)?",
    "eval_const_hir_with_locals(value, constants, local_constants)?",
)
segment = segment.replace(
    "eval_const_hir_resolved(left, constants)?",
    "eval_const_hir_with_locals(left, constants, local_constants)?",
)
segment = segment.replace(
    "eval_const_hir_resolved(right, constants)?",
    "eval_const_hir_with_locals(right, constants, local_constants)?",
)
text = text[:segment_start] + segment + text[segment_end:]
path.write_text(text)


# Tests: local const must really be compile-time and usable in local type positions.
tests = Path("crates/forge-frontend/tests/typecheck.rs")
text = tests.read_text()
addition = r'''

#[test]
fn local_consts_are_checked_retained_and_usable_in_array_lengths() {
    let output = check(
        r#"
        module test.local_const;
        fn main() -> u32 {
            const WIDTH: usize = 2;
            const COUNT: usize = WIDTH * 2;
            val values: [u32; COUNT] = [1u32, 2u32, 3u32, 4u32];
            val [a, b, c, d] = values;
            return a + b + c + d;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.get(&DefId(0)).expect("main body");
    assert!(body
        .local_constants
        .values()
        .any(|value| *value == forge_frontend::ConstValue::Integer { value: 2 }));
    assert!(body
        .local_constants
        .values()
        .any(|value| *value == forge_frontend::ConstValue::Integer { value: 4 }));
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array { length: Some(4), .. }
    )));

    let runtime = check(
        r#"
        module test.local_const_runtime;
        fn runtime() -> usize { return 4usize; }
        fn main() -> i32 {
            const BAD: usize = runtime();
            return 0;
        }
        "#,
    );
    assert!(has(&runtime, "const/eval"), "{:?}", runtime.diagnostics);
}
'''
if "fn local_consts_are_checked_retained_and_usable_in_array_lengths()" not in text:
    tests.write_text(text + addition)


# Document the semantic boundary: const is erased to a retained value, not to val.
arch = Path("docs/compiler-architecture.md")
text = arch.read_text()
old = '''Compile-time constants use a deliberately restricted semantic evaluator over pure literal/unary/binary expressions and references to module `const` definitions. Evaluation is memoized by `DefId`, detects dependency cycles, and produces retained `ConstValue`s used by array lengths and explicit enum discriminants. Forge v1 does not execute arbitrary functions at compile time and has no general comptime interpreter.
'''
new = '''Compile-time constants use a deliberately restricted semantic evaluator over pure literal/unary/binary expressions and references to compile-time `const` bindings. Module constants are memoized by `DefId` with dependency-cycle detection; local constants are retained by `LocalId` in typed bodies. These retained `ConstValue`s feed array lengths and explicit enum discriminants, so FIR never re-evaluates source expressions. Forge v1 does not execute arbitrary functions at compile time and has no general comptime interpreter.
'''
if old in text:
    text = text.replace(old, new, 1)
arch.write_text(text)

spec = Path("docs/forge-v1-spec.md")
text = spec.read_text()
old = '''Forge v1 constant evaluation is intentionally restricted. Constant expressions may use scalar literals, pure unary/binary operators, and references to other module `const` definitions. Constant dependencies may be forward references but cycles are rejected. Arbitrary function calls and general compile-time execution are not part of v1.
'''
new = '''Forge v1 constant evaluation is intentionally restricted. Constant expressions may use scalar literals, pure unary/binary operators, and references to other compile-time `const` bindings. Module constant dependencies may be forward references but cycles are rejected; local constants may refer to earlier local constants in lexical scope. Arbitrary function calls and general compile-time execution are not part of v1. V1 compile-time `const` bindings use a single binding name rather than destructuring patterns.
'''
if old in text:
    text = text.replace(old, new, 1)
spec.write_text(text)

print("local const semantic cleanup patch applied")
