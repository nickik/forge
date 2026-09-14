from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement target in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


# ---- type checker ---------------------------------------------------------
path = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = path.read_text()

needle = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDiagnostic {
'''
insert = '''#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "const", rename_all = "snake_case")]
pub enum ConstValue {
    Integer { value: i128 },
    Bool { value: bool },
    Char { value: char },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDiagnostic {
'''
if needle not in text:
    raise SystemExit("TypeDiagnostic insertion point missing")
text = text.replace(needle, insert, 1)

old = '''    ResolvedCall {
        target: DefId,
        method: bool,
        hir: HirExpr,
    },
    OptionalPromote {
'''
new = '''    ResolvedCall {
        target: DefId,
        method: bool,
        hir: HirExpr,
    },
    ResolvedTry {
        source_error: Ty,
        target_error: Ty,
        hir: HirExpr,
    },
    OptionalPromote {
'''
if old not in text:
    raise SystemExit("TypedExprKind insertion point missing")
text = text.replace(old, new, 1)

old = '''pub struct TypeCheckOutput {
    pub functions: BTreeMap<DefId, TypedBody>,
    pub global_types: BTreeMap<DefId, Ty>,
    pub metadata: MetadataTable,
    pub diagnostics: Vec<TypeDiagnostic>,
}
'''
new = '''pub struct TypeCheckOutput {
    pub functions: BTreeMap<DefId, TypedBody>,
    pub global_types: BTreeMap<DefId, Ty>,
    pub constants: BTreeMap<DefId, ConstValue>,
    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,
    pub metadata: MetadataTable,
    pub diagnostics: Vec<TypeDiagnostic>,
}
'''
if old not in text:
    raise SystemExit("TypeCheckOutput block missing")
text = text.replace(old, new, 1)

old = '''    let mut output = TypeCheckOutput {
        metadata: module.metadata.clone(),
        ..TypeCheckOutput::default()
    };
    validate_declaration_array_lengths(source, &mut output.diagnostics);
    let env = ModuleTypeEnv::build(source, module);
'''
new = '''    let mut output = TypeCheckOutput {
        metadata: module.metadata.clone(),
        ..TypeCheckOutput::default()
    };
    let (constant_values, constant_diagnostics) =
        ConstEvaluator::new(source, bodies).evaluate_all();
    output.constants = constant_values.clone();
    output.diagnostics.extend(constant_diagnostics);
    validate_declaration_array_lengths(
        source,
        module,
        &constant_values,
        &mut output.diagnostics,
    );
    let env = ModuleTypeEnv::build(source, module, &constant_values);
'''
if old not in text:
    raise SystemExit("type_check_module prelude missing")
text = text.replace(old, new, 1)

old = '''    for explicit in &bodies.enum_values {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let actual = checker.check_expr(&explicit.value, None);
        if !is_integer_like(&actual) {
            checker.diagnostic(
                explicit.value.span,
                "type/enum-value",
                format!("enum value must be an integer constant, found {actual:?}"),
            );
        }
        if eval_const_int_hir(&explicit.value).is_none() {
            checker.diagnostic(
                explicit.value.span,
                "type/enum-value",
                "enum value must be a compile-time integer expression",
            );
        }
    }
'''
new = '''    for explicit in &bodies.enum_values {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let actual = checker.check_expr(&explicit.value, None);
        match eval_const_hir_resolved(&explicit.value, &constant_values) {
            Ok(ConstValue::Integer { value }) if is_integer_like(&actual) => {
                output
                    .enum_values
                    .entry(explicit.owner)
                    .or_default()
                    .insert(explicit.variant.clone(), value);
            }
            Ok(value) => checker.diagnostic(
                explicit.value.span,
                "type/enum-value",
                format!("enum value must be an integer constant, found {value:?}"),
            ),
            Err(error) => checker.diagnostic(
                explicit.value.span,
                "type/enum-value",
                format!("enum value must be a compile-time integer expression: {}", error.message),
            ),
        }
    }
'''
if old not in text:
    raise SystemExit("enum value checking block missing")
text = text.replace(old, new, 1)

old = '''    for (owner, global) in &bodies.globals {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let value_ty = checker.check_expr(&global.value, None);
        let ty = if let Some(annotation) = &global.ty {
            let expected = env.lower_hir_type(annotation);
            checker.require_assignable(global.value.span, &expected, &value_ty, "type/mismatch");
            expected
        } else {
            checker.materialize_literal(global.value.span, value_ty)
        };
        output.global_types.insert(*owner, ty);
    }
'''
new = '''    for (owner, global) in &bodies.globals {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let expected = global.ty.as_ref().map(|annotation| env.lower_hir_type(annotation));
        let value_ty = checker.check_expr(&global.value, expected.as_ref());
        let ty = if let Some(expected) = expected {
            checker.require_assignable(global.value.span, &expected, &value_ty, "type/mismatch");
            expected
        } else {
            checker.materialize_literal(global.value.span, value_ty)
        };
        output.global_types.insert(*owner, ty);
    }
'''
if old not in text:
    raise SystemExit("global checking block missing")
text = text.replace(old, new, 1)

old = '''struct ModuleTypeEnv {
    types: BTreeMap<DefId, TypeInfo>,
    functions: BTreeMap<DefId, FunctionSig>,
    methods: BTreeMap<(DefId, String), DefId>,
}

impl ModuleTypeEnv {
    fn build(source: &ast::SourceFile, module: &HirModule) -> Self {
        let mut env = Self {
            types: BTreeMap::new(),
            functions: BTreeMap::new(),
            methods: BTreeMap::new(),
        };
'''
new = '''struct ModuleTypeEnv {
    types: BTreeMap<DefId, TypeInfo>,
    functions: BTreeMap<DefId, FunctionSig>,
    methods: BTreeMap<(DefId, String), DefId>,
    globals: BTreeMap<DefId, Ty>,
    constants: BTreeMap<DefId, ConstValue>,
}

impl ModuleTypeEnv {
    fn build(
        source: &ast::SourceFile,
        module: &HirModule,
        constants: &BTreeMap<DefId, ConstValue>,
    ) -> Self {
        let mut env = Self {
            types: BTreeMap::new(),
            functions: BTreeMap::new(),
            methods: BTreeMap::new(),
            globals: BTreeMap::new(),
            constants: constants.clone(),
        };
'''
if old not in text:
    raise SystemExit("ModuleTypeEnv block missing")
text = text.replace(old, new, 1)

# Add declared global types before function signatures.
needle = '''        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            if let DeclKind::Function(function) = &declaration.kind.kind {
'''
insert = '''        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            if let DeclKind::Global(value) = &declaration.kind.kind {
                if let Some(annotation) = &value.ty {
                    let ty = env.lower_ast_type(annotation, module);
                    env.globals.insert(id, ty);
                }
            }
        }

        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            if let DeclKind::Function(function) = &declaration.kind.kind {
'''
if needle not in text:
    raise SystemExit("function pass insertion point missing")
text = text.replace(needle, insert, 1)

text = text.replace(
'''            ast::TypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
                length: eval_const_usize_ast(length),
            },
''',
'''            ast::TypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
                length: eval_const_ast_resolved(length, module, &self.constants)
                    .ok()
                    .and_then(const_value_to_u64),
            },
''',
1,
)
text = text.replace(
'''            HirTypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
                length: eval_const_usize_hir(length),
            },
''',
'''            HirTypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
                length: eval_const_hir_resolved(length, &self.constants)
                    .ok()
                    .and_then(const_value_to_u64),
            },
''',
1,
)

old = '''            ResolvedName::Def(id) => self
                .env
                .functions
                .get(&id)
                .map(|sig| Ty::Function {
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    result: Box::new(sig.result.clone()),
                    named_arguments: sig.named_arguments,
                })
                .unwrap_or(Ty::Unknown),
'''
new = '''            ResolvedName::Def(id) => {
                if let Some(sig) = self.env.functions.get(&id) {
                    Ty::Function {
                        params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                        result: Box::new(sig.result.clone()),
                        named_arguments: sig.named_arguments,
                    }
                } else if let Some(ty) = self.env.globals.get(&id) {
                    ty.clone()
                } else if let Some(value) = self.env.constants.get(&id) {
                    const_value_ty(value)
                } else {
                    Ty::Unknown
                }
            }
'''
if old not in text:
    raise SystemExit("type_of_name Def arm missing")
text = text.replace(old, new, 1)

old = '''    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut resolved_call: Option<(DefId, bool)> = None;
        let mut ty = match &expr.kind {
'''
new = '''    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut resolved_call: Option<(DefId, bool)> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;
        let mut ty = match &expr.kind {
'''
if old not in text:
    raise SystemExit("check_expr prelude missing")
text = text.replace(old, new, 1)

old = '''            HirExprKind::Try { value } => match self.check_expr(value, None) {
                Ty::Result { ok, .. } => *ok,
                other => {
                    self.diagnostic(
                        expr.span,
                        "type/mismatch",
                        format!("`?` requires Result, found {other:?}"),
                    );
                    Ty::Error
                }
            },
'''
new = '''            HirExprKind::Try { value } => match self.check_expr(value, None) {
                Ty::Result { ok, error } => {
                    let ok = *ok;
                    let source_error = *error;
                    match self.expected_return.clone() {
                        Ty::Result { error, .. } => {
                            let target_error = *error;
                            if self.is_assignable(&target_error, &source_error) {
                                resolved_try = Some((source_error, target_error));
                            } else {
                                self.diagnostic(
                                    expr.span,
                                    "try/error-type",
                                    format!(
                                        "cannot propagate error type {source_error:?}; enclosing function returns Result with error type {target_error:?}"
                                    ),
                                );
                            }
                        }
                        other => self.diagnostic(
                            expr.span,
                            "try/context",
                            format!(
                                "`?` requires the enclosing function or closure to return Result, found {other:?}"
                            ),
                        ),
                    }
                    ok
                }
                other => {
                    self.diagnostic(
                        expr.span,
                        "try/operand",
                        format!("`?` requires a Result operand, found {other:?}"),
                    );
                    Ty::Error
                }
            },
'''
if old not in text:
    raise SystemExit("Try checking block missing")
text = text.replace(old, new, 1)

old = '''        let kind = resolved_call
            .map(|(target, method)| TypedExprKind::ResolvedCall {
                target,
                method,
                hir: expr.clone(),
            })
            .unwrap_or_else(|| TypedExprKind::Source { hir: expr.clone() });
'''
new = '''        let kind = if let Some((source_error, target_error)) = resolved_try {
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                hir: expr.clone(),
            }
        } else if let Some((target, method)) = resolved_call {
            TypedExprKind::ResolvedCall {
                target,
                method,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
'''
if old not in text:
    raise SystemExit("typed expression kind selection missing")
text = text.replace(old, new, 1)

# Replace the old ad-hoc constant helpers with one semantic subsystem.
start = text.find("fn array_type_has_unknown_length")
end = text.find("fn arg_value", start)
if start < 0 or end < 0:
    raise SystemExit("constant helper replacement markers missing")
new_helpers = r'''fn array_type_has_unknown_length(ty: &Ty) -> bool {
    match ty {
        Ty::Array { element, length } => length.is_none() || array_type_has_unknown_length(element),
        Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } | Ty::Optional { inner } => {
            array_type_has_unknown_length(inner)
        }
        Ty::Slice { element, .. } => array_type_has_unknown_length(element),
        Ty::Result { ok, error } => {
            array_type_has_unknown_length(ok) || array_type_has_unknown_length(error)
        }
        Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
            params.iter().any(array_type_has_unknown_length)
                || array_type_has_unknown_length(result)
        }
        _ => false,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConstEvalError {
    span: Span,
    message: String,
}

#[derive(Debug, Clone)]
enum ConstState {
    Evaluating,
    Evaluated(ConstValue),
    Failed(ConstEvalError),
}

struct ConstEvaluator<'a> {
    source: &'a ast::SourceFile,
    bodies: &'a BodyHirOutput,
    states: BTreeMap<DefId, ConstState>,
    stack: Vec<DefId>,
}

impl<'a> ConstEvaluator<'a> {
    fn new(source: &'a ast::SourceFile, bodies: &'a BodyHirOutput) -> Self {
        Self {
            source,
            bodies,
            states: BTreeMap::new(),
            stack: Vec::new(),
        }
    }

    fn evaluate_all(mut self) -> (BTreeMap<DefId, ConstValue>, Vec<TypeDiagnostic>) {
        let ids = self
            .source
            .declarations
            .iter()
            .enumerate()
            .filter_map(|(index, declaration)| match &declaration.kind.kind {
                DeclKind::Global(value) if matches!(value.binding, ast::BindingKind::Const) => {
                    Some(DefId(index as u32))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut diagnostics = Vec::new();
        for id in ids {
            if let Err(error) = self.eval_def(id) {
                diagnostics.push(TypeDiagnostic {
                    span: error.span,
                    code: "const/eval".into(),
                    message: error.message,
                });
            }
        }
        let values = self
            .states
            .into_iter()
            .filter_map(|(id, state)| match state {
                ConstState::Evaluated(value) => Some((id, value)),
                ConstState::Evaluating | ConstState::Failed(_) => None,
            })
            .collect();
        (values, diagnostics)
    }

    fn eval_def(&mut self, id: DefId) -> Result<ConstValue, ConstEvalError> {
        match self.states.get(&id).cloned() {
            Some(ConstState::Evaluated(value)) => return Ok(value),
            Some(ConstState::Failed(error)) => return Err(error),
            Some(ConstState::Evaluating) => {
                let begin = self.stack.iter().position(|candidate| *candidate == id).unwrap_or(0);
                let mut names = self.stack[begin..]
                    .iter()
                    .map(|id| self.const_name(*id))
                    .collect::<Vec<_>>();
                names.push(self.const_name(id));
                return Err(ConstEvalError {
                    span: self.declaration_span(id),
                    message: format!("constant evaluation cycle: {}", names.join(" -> ")),
                });
            }
            None => {}
        }

        let Some(declaration) = self.source.declarations.get(id.0 as usize) else {
            return Err(ConstEvalError {
                span: Span::new(0, 0),
                message: format!("unknown constant definition {id:?}"),
            });
        };
        let DeclKind::Global(global) = &declaration.kind.kind else {
            return Err(ConstEvalError {
                span: declaration.span,
                message: "referenced definition is not a constant".into(),
            });
        };
        if !matches!(global.binding, ast::BindingKind::Const) {
            return Err(ConstEvalError {
                span: declaration.span,
                message: "referenced global is not declared `const`".into(),
            });
        }
        if !matches!(global.pattern.kind, ast::PatternKind::Binding { .. }) {
            return Err(ConstEvalError {
                span: global.pattern.span,
                message: "Forge v1 constant evaluation requires a single-name global `const` binding"
                    .into(),
            });
        }
        let Some(expr) = self.bodies.globals.get(&id).map(|body| body.value.clone()) else {
            return Err(ConstEvalError {
                span: declaration.span,
                message: "constant body was not lowered into HIR".into(),
            });
        };

        self.states.insert(id, ConstState::Evaluating);
        self.stack.push(id);
        let result = self.eval_expr(&expr);
        self.stack.pop();
        match &result {
            Ok(value) => {
                self.states.insert(id, ConstState::Evaluated(value.clone()));
            }
            Err(error) => {
                self.states.insert(id, ConstState::Failed(error.clone()));
            }
        }
        result
    }

    fn eval_expr(&mut self, expr: &HirExpr) -> Result<ConstValue, ConstEvalError> {
        match &expr.kind {
            HirExprKind::Integer { text } => parse_integer_value(text)
                .map(|value| ConstValue::Integer { value })
                .ok_or_else(|| ConstEvalError {
                    span: expr.span,
                    message: format!("invalid integer constant `{text}`"),
                }),
            HirExprKind::Bool { value } => Ok(ConstValue::Bool { value: *value }),
            HirExprKind::Character { value } => Ok(ConstValue::Char { value: *value }),
            HirExprKind::Name { reference } if reference.tail.is_empty() => match reference.root {
                ResolvedName::Def(id) => self.eval_def(id),
                _ => Err(ConstEvalError {
                    span: expr.span,
                    message: "constant expression may reference only module `const` definitions"
                        .into(),
                }),
            },
            HirExprKind::Unary { op, value } => {
                let value = self.eval_expr(value)?;
                apply_const_unary(expr.span, *op, value)
            }
            HirExprKind::Binary { op, left, right } => {
                let left = self.eval_expr(left)?;
                if matches!(op, BinaryOp::LogicalAnd) && matches!(left, ConstValue::Bool { value: false }) {
                    return Ok(ConstValue::Bool { value: false });
                }
                if matches!(op, BinaryOp::LogicalOr) && matches!(left, ConstValue::Bool { value: true }) {
                    return Ok(ConstValue::Bool { value: true });
                }
                let right = self.eval_expr(right)?;
                apply_const_binary(expr.span, *op, left, right)
            }
            _ => Err(ConstEvalError {
                span: expr.span,
                message: "expression is not allowed in a Forge v1 compile-time constant".into(),
            }),
        }
    }

    fn declaration_span(&self, id: DefId) -> Span {
        self.source
            .declarations
            .get(id.0 as usize)
            .map(|declaration| declaration.span)
            .unwrap_or_else(|| Span::new(0, 0))
    }

    fn const_name(&self, id: DefId) -> String {
        self.source
            .declarations
            .get(id.0 as usize)
            .and_then(|declaration| match &declaration.kind.kind {
                DeclKind::Global(global) => match &global.pattern.kind {
                    ast::PatternKind::Binding { name, .. } => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .unwrap_or_else(|| format!("const#{:?}", id.0))
    }
}

fn eval_const_ast_resolved(
    expr: &ast::Expr,
    module: &HirModule,
    constants: &BTreeMap<DefId, ConstValue>,
) -> Result<ConstValue, ConstEvalError> {
    match &expr.kind {
        ast::ExprKind::Integer { text } => parse_integer_value(text)
            .map(|value| ConstValue::Integer { value })
            .ok_or_else(|| ConstEvalError {
                span: expr.span,
                message: format!("invalid integer constant `{text}`"),
            }),
        ast::ExprKind::Bool { value } => Ok(ConstValue::Bool { value: *value }),
        ast::ExprKind::Character { value } => Ok(ConstValue::Char { value: *value }),
        ast::ExprKind::Path { path } if path.segments.len() == 1 => {
            let name = &path.segments[0];
            let id = module
                .symbols
                .get(name)
                .and_then(|symbols| symbols.value_def)
                .ok_or_else(|| ConstEvalError {
                    span: expr.span,
                    message: format!("`{name}` does not resolve to a module constant"),
                })?;
            constants.get(&id).cloned().ok_or_else(|| ConstEvalError {
                span: expr.span,
                message: format!("`{name}` is not an evaluable module `const`"),
            })
        }
        ast::ExprKind::Unary { op, value } => apply_const_unary(
            expr.span,
            *op,
            eval_const_ast_resolved(value, module, constants)?,
        ),
        ast::ExprKind::Binary { op, left, right } => {
            let left = eval_const_ast_resolved(left, module, constants)?;
            if matches!(op, BinaryOp::LogicalAnd) && matches!(left, ConstValue::Bool { value: false }) {
                return Ok(ConstValue::Bool { value: false });
            }
            if matches!(op, BinaryOp::LogicalOr) && matches!(left, ConstValue::Bool { value: true }) {
                return Ok(ConstValue::Bool { value: true });
            }
            let right = eval_const_ast_resolved(right, module, constants)?;
            apply_const_binary(expr.span, *op, left, right)
        }
        _ => Err(ConstEvalError {
            span: expr.span,
            message: "expression is not allowed in a Forge v1 compile-time constant".into(),
        }),
    }
}

fn eval_const_hir_resolved(
    expr: &HirExpr,
    constants: &BTreeMap<DefId, ConstValue>,
) -> Result<ConstValue, ConstEvalError> {
    match &expr.kind {
        HirExprKind::Integer { text } => parse_integer_value(text)
            .map(|value| ConstValue::Integer { value })
            .ok_or_else(|| ConstEvalError {
                span: expr.span,
                message: format!("invalid integer constant `{text}`"),
            }),
        HirExprKind::Bool { value } => Ok(ConstValue::Bool { value: *value }),
        HirExprKind::Character { value } => Ok(ConstValue::Char { value: *value }),
        HirExprKind::Name { reference } if reference.tail.is_empty() => match reference.root {
            ResolvedName::Def(id) => constants.get(&id).cloned().ok_or_else(|| ConstEvalError {
                span: expr.span,
                message: format!("definition {id:?} is not an evaluable module `const`"),
            }),
            _ => Err(ConstEvalError {
                span: expr.span,
                message: "constant expression may reference only module `const` definitions".into(),
            }),
        },
        HirExprKind::Unary { op, value } => apply_const_unary(
            expr.span,
            *op,
            eval_const_hir_resolved(value, constants)?,
        ),
        HirExprKind::Binary { op, left, right } => {
            let left = eval_const_hir_resolved(left, constants)?;
            if matches!(op, BinaryOp::LogicalAnd) && matches!(left, ConstValue::Bool { value: false }) {
                return Ok(ConstValue::Bool { value: false });
            }
            if matches!(op, BinaryOp::LogicalOr) && matches!(left, ConstValue::Bool { value: true }) {
                return Ok(ConstValue::Bool { value: true });
            }
            let right = eval_const_hir_resolved(right, constants)?;
            apply_const_binary(expr.span, *op, left, right)
        }
        _ => Err(ConstEvalError {
            span: expr.span,
            message: "expression is not allowed in a Forge v1 compile-time constant".into(),
        }),
    }
}

fn apply_const_unary(
    span: Span,
    op: UnaryOp,
    value: ConstValue,
) -> Result<ConstValue, ConstEvalError> {
    match (op, value) {
        (UnaryOp::Neg, ConstValue::Integer { value }) => value
            .checked_neg()
            .map(|value| ConstValue::Integer { value })
            .ok_or_else(|| ConstEvalError {
                span,
                message: "integer constant overflow".into(),
            }),
        (UnaryOp::BitNot, ConstValue::Integer { value }) => {
            Ok(ConstValue::Integer { value: !value })
        }
        (UnaryOp::Not, ConstValue::Bool { value }) => Ok(ConstValue::Bool { value: !value }),
        (_, value) => Err(ConstEvalError {
            span,
            message: format!("invalid constant unary operation on {value:?}"),
        }),
    }
}

fn apply_const_binary(
    span: Span,
    op: BinaryOp,
    left: ConstValue,
    right: ConstValue,
) -> Result<ConstValue, ConstEvalError> {
    match (left, right) {
        (ConstValue::Integer { value: left }, ConstValue::Integer { value: right }) => {
            let integer = |value| Ok(ConstValue::Integer { value });
            let boolean = |value| Ok(ConstValue::Bool { value });
            match op {
                BinaryOp::Add => left.checked_add(right).map_or_else(
                    || Err(const_arithmetic_error(span)),
                    integer,
                ),
                BinaryOp::Sub => left.checked_sub(right).map_or_else(
                    || Err(const_arithmetic_error(span)),
                    integer,
                ),
                BinaryOp::Mul => left.checked_mul(right).map_or_else(
                    || Err(const_arithmetic_error(span)),
                    integer,
                ),
                BinaryOp::Div => left.checked_div(right).map_or_else(
                    || Err(const_arithmetic_error(span)),
                    integer,
                ),
                BinaryOp::Rem => left.checked_rem(right).map_or_else(
                    || Err(const_arithmetic_error(span)),
                    integer,
                ),
                BinaryOp::BitAnd => integer(left & right),
                BinaryOp::BitXor => integer(left ^ right),
                BinaryOp::BitOr => integer(left | right),
                BinaryOp::ShiftLeft => {
                    let shift = u32::try_from(right).map_err(|_| ConstEvalError {
                        span,
                        message: "constant shift count is outside the supported range".into(),
                    })?;
                    left.checked_shl(shift).map_or_else(
                        || Err(const_arithmetic_error(span)),
                        integer,
                    )
                }
                BinaryOp::ShiftRight => {
                    let shift = u32::try_from(right).map_err(|_| ConstEvalError {
                        span,
                        message: "constant shift count is outside the supported range".into(),
                    })?;
                    left.checked_shr(shift).map_or_else(
                        || Err(const_arithmetic_error(span)),
                        integer,
                    )
                }
                BinaryOp::Eq => boolean(left == right),
                BinaryOp::NotEq => boolean(left != right),
                BinaryOp::Less => boolean(left < right),
                BinaryOp::LessEq => boolean(left <= right),
                BinaryOp::Greater => boolean(left > right),
                BinaryOp::GreaterEq => boolean(left >= right),
                _ => Err(ConstEvalError {
                    span,
                    message: format!("operator {op:?} is not valid for integer constants"),
                }),
            }
        }
        (ConstValue::Bool { value: left }, ConstValue::Bool { value: right }) => {
            let value = match op {
                BinaryOp::LogicalAnd => left && right,
                BinaryOp::LogicalOr => left || right,
                BinaryOp::LogicalXor => left ^ right,
                BinaryOp::Eq => left == right,
                BinaryOp::NotEq => left != right,
                _ => {
                    return Err(ConstEvalError {
                        span,
                        message: format!("operator {op:?} is not valid for bool constants"),
                    })
                }
            };
            Ok(ConstValue::Bool { value })
        }
        (ConstValue::Char { value: left }, ConstValue::Char { value: right }) => {
            let value = match op {
                BinaryOp::Eq => left == right,
                BinaryOp::NotEq => left != right,
                BinaryOp::Less => left < right,
                BinaryOp::LessEq => left <= right,
                BinaryOp::Greater => left > right,
                BinaryOp::GreaterEq => left >= right,
                _ => {
                    return Err(ConstEvalError {
                        span,
                        message: format!("operator {op:?} is not valid for char constants"),
                    })
                }
            };
            Ok(ConstValue::Bool { value })
        }
        (left, right) => Err(ConstEvalError {
            span,
            message: format!("constant operands have incompatible kinds: {left:?} and {right:?}"),
        }),
    }
}

fn const_arithmetic_error(span: Span) -> ConstEvalError {
    ConstEvalError {
        span,
        message: "constant arithmetic overflow or invalid division/shift".into(),
    }
}

fn const_value_to_u64(value: ConstValue) -> Option<u64> {
    match value {
        ConstValue::Integer { value } => u64::try_from(value).ok(),
        ConstValue::Bool { .. } | ConstValue::Char { .. } => None,
    }
}

fn const_value_ty(value: &ConstValue) -> Ty {
    match value {
        ConstValue::Integer { .. } => Ty::IntLiteral,
        ConstValue::Bool { .. } => Ty::Bool,
        ConstValue::Char { .. } => Ty::Char,
    }
}

fn validate_declaration_array_lengths(
    source: &ast::SourceFile,
    module: &HirModule,
    constants: &BTreeMap<DefId, ConstValue>,
    diagnostics: &mut Vec<TypeDiagnostic>,
) {
    fn validate_type(
        ty: &ast::TypeNode,
        module: &HirModule,
        constants: &BTreeMap<DefId, ConstValue>,
        diagnostics: &mut Vec<TypeDiagnostic>,
    ) {
        match &ty.kind {
            ast::TypeKind::Array { element, length } => {
                let valid = eval_const_ast_resolved(length, module, constants)
                    .ok()
                    .and_then(const_value_to_u64)
                    .is_some();
                if !valid {
                    diagnostics.push(TypeDiagnostic {
                        span: length.span,
                        code: "type/array-length".into(),
                        message: "array length must be a non-negative compile-time integer".into(),
                    });
                }
                validate_type(element, module, constants, diagnostics);
            }
            ast::TypeKind::Pointer { inner, .. }
            | ast::TypeKind::Reference { inner, .. }
            | ast::TypeKind::Optional { inner } => {
                validate_type(inner, module, constants, diagnostics)
            }
            ast::TypeKind::Slice { element, .. } => {
                validate_type(element, module, constants, diagnostics)
            }
            ast::TypeKind::Result { ok, error } => {
                validate_type(ok, module, constants, diagnostics);
                validate_type(error, module, constants, diagnostics);
            }
            ast::TypeKind::Function { params, result }
            | ast::TypeKind::Closure { params, result } => {
                for param in params {
                    validate_type(param, module, constants, diagnostics);
                }
                validate_type(result, module, constants, diagnostics);
            }
            ast::TypeKind::Named { .. } => {}
        }
    }

    for declaration in &source.declarations {
        match &declaration.kind.kind {
            DeclKind::Function(function) => {
                for param in &function.params {
                    validate_type(&param.ty, module, constants, diagnostics);
                }
                if let Some(result) = &function.return_type {
                    validate_type(result, module, constants, diagnostics);
                }
            }
            DeclKind::Struct(value) => {
                for field in &value.fields {
                    validate_type(&field.ty, module, constants, diagnostics);
                }
            }
            DeclKind::Tagged(value) => {
                for variant in &value.variants {
                    for field in &variant.fields {
                        validate_type(&field.ty, module, constants, diagnostics);
                    }
                }
            }
            DeclKind::BitStruct(value) => {
                validate_type(&value.storage, module, constants, diagnostics)
            }
            DeclKind::Distinct(value) => {
                validate_type(&value.underlying, module, constants, diagnostics)
            }
            DeclKind::TypeAlias(value) => {
                validate_type(&value.target, module, constants, diagnostics)
            }
            DeclKind::Impl(value) => {
                for method in &value.methods {
                    for param in &method.function.params {
                        validate_type(&param.ty, module, constants, diagnostics);
                    }
                    if let Some(result) = &method.function.return_type {
                        validate_type(result, module, constants, diagnostics);
                    }
                }
            }
            DeclKind::Global(value) => {
                if let Some(ty) = &value.ty {
                    validate_type(ty, module, constants, diagnostics);
                }
            }
            DeclKind::Enum(_) => {}
        }
    }
}

fn parse_integer_value(text: &str) -> Option<i128> {
    let mut raw = text.replace('_', "");
    for suffix in [
        "usize", "isize", "u64", "i64", "u32", "i32", "u16", "i16", "u8", "i8",
    ] {
        if raw.ends_with(suffix) {
            raw.truncate(raw.len() - suffix.len());
            break;
        }
    }
    let (radix, digits) = if let Some(rest) = raw.strip_prefix("0x") {
        (16, rest)
    } else if let Some(rest) = raw.strip_prefix("0b") {
        (2, rest)
    } else if let Some(rest) = raw.strip_prefix("0o") {
        (8, rest)
    } else {
        (10, raw.as_str())
    };
    i128::from_str_radix(digits, radix).ok()
}

'''
text = text[:start] + new_helpers + text[end:]
path.write_text(text)

# ---- lib exports ----------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/lib.rs",
    '''pub use typecheck::{
    type_check_module, IntWidth, Ty, TypeCheckOutput, TypeDiagnostic, TypedBody, TypedExpr,
    TypedExprKind,
};
''',
    '''pub use typecheck::{
    type_check_module, ConstValue, IntWidth, Ty, TypeCheckOutput, TypeDiagnostic, TypedBody,
    TypedExpr, TypedExprKind,
};
''',
)

# ---- regression tests ----------------------------------------------------
tests = Path("crates/forge-frontend/tests/typecheck.rs")
text = tests.read_text()
addition = r'''

#[test]
fn result_try_is_resolved_and_requires_compatible_enclosing_result() {
    let valid = check(
        r#"
        module test.try_valid;
        fn source() -> Result[u32, u8] { return source(); }
        fn propagate() -> Result[u32, u8] {
            val value = source()?;
            return source();
        }
        "#,
    );
    assert!(valid.diagnostics.is_empty(), "{:?}", valid.diagnostics);
    let body = valid.functions.get(&DefId(1)).expect("propagate body");
    assert!(body.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedTry {
            source_error: Ty::Int { signed: false, width: IntWidth::W8 },
            target_error: Ty::Int { signed: false, width: IntWidth::W8 },
            ..
        }
    )));

    let mismatch = check(
        r#"
        module test.try_mismatch;
        fn source() -> Result[u32, u8] { return source(); }
        fn target() -> Result[u32, u16] {
            val value = source()?;
            return target();
        }
        "#,
    );
    assert!(has(&mismatch, "try/error-type"), "{:?}", mismatch.diagnostics);

    let plain = check(
        r#"
        module test.try_plain;
        fn source() -> Result[u32, u8] { return source(); }
        fn target() -> u32 {
            val value = source()?;
            return value;
        }
        "#,
    );
    assert!(has(&plain, "try/context"), "{:?}", plain.diagnostics);

    let optional = check(
        r#"
        module test.try_optional;
        fn target(value: u32?) -> u32 {
            return value?;
        }
        "#,
    );
    assert!(has(&optional, "try/operand"), "{:?}", optional.diagnostics);
}

#[test]
fn constants_feed_array_lengths_and_enum_values() {
    let output = check(
        r#"
        module test.constants;
        const WIDTH: usize = 2;
        const COUNT: usize = WIDTH * 2;
        enum Code {
            Small = WIDTH,
            Large = COUNT + 1,
        }
        fn total(values: [u32; COUNT]) -> u32 {
            val [a, b, c, d] = values;
            return a + b + c + d;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        output.constants.get(&DefId(0)),
        Some(&forge_frontend::ConstValue::Integer { value: 2 })
    );
    assert_eq!(
        output.constants.get(&DefId(1)),
        Some(&forge_frontend::ConstValue::Integer { value: 4 })
    );
    assert_eq!(
        output.enum_values.get(&DefId(2)).and_then(|values| values.get("Small")),
        Some(&2)
    );
    assert_eq!(
        output.enum_values.get(&DefId(2)).and_then(|values| values.get("Large")),
        Some(&5)
    );
    let body = output.functions.get(&DefId(3)).expect("total body");
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array { length: Some(4), .. }
    )));
}

#[test]
fn constant_evaluation_rejects_cycles_and_runtime_calls() {
    let cycle = check(
        r#"
        module test.const_cycle;
        const A: usize = B + 1;
        const B: usize = A + 1;
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&cycle, "const/eval"), "{:?}", cycle.diagnostics);
    assert!(cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("A -> B -> A")));

    let runtime = check(
        r#"
        module test.const_runtime;
        fn runtime() -> usize { return 1usize; }
        const BAD: usize = runtime();
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&runtime, "const/eval"), "{:?}", runtime.diagnostics);
}

#[test]
fn array_length_rejects_non_const_global() {
    let output = check(
        r#"
        module test.array_non_const;
        val COUNT: usize = 4usize;
        fn total(values: [u32; COUNT]) -> u32 { return 0u32; }
        "#,
    );
    assert!(has(&output, "type/array-length"), "{:?}", output.diagnostics);
}
'''
if "fn result_try_is_resolved_and_requires_compatible_enclosing_result()" not in text:
    tests.write_text(text + addition)

# ---- documentation -------------------------------------------------------
arch = Path("docs/compiler-architecture.md")
text = arch.read_text()
needle = '''`Option` and `Result` are compiler-recognized type constructors despite v1 not exposing user-defined generics.
'''
replacement = '''`Option` and `Result` are compiler-recognized type constructors despite v1 not exposing user-defined generics.

Postfix `?` is resolved during type checking: its operand must be `Result[T, Ein]`, the enclosing function or closure must return `Result[R, Eout]`, and `Ein` must be assignable to `Eout`. Typed HIR records the resolved propagation edge explicitly so FIR never has to reconstruct `?` semantics from syntax.

Compile-time constants use a deliberately restricted semantic evaluator over pure literal/unary/binary expressions and references to module `const` definitions. Evaluation is memoized by `DefId`, detects dependency cycles, and produces retained `ConstValue`s used by array lengths and explicit enum discriminants. Forge v1 does not execute arbitrary functions at compile time and has no general comptime interpreter.
'''
if needle in text and "Compile-time constants use a deliberately restricted semantic evaluator" not in text:
    text = text.replace(needle, replacement, 1)
old = '''### Bitstruct v1 proposal (not yet normative)

Keep bitstructs simple: restrict storage to unsigned fixed-width integers; map each field to the smallest ordinary unsigned integer type that can hold its declared width; never create source-level 3-bit/5-bit integer types; compile-time-known out-of-range writes are errors and dynamic writes are checked rather than truncated. Field ordering and bit numbering still need an explicit language decision before implementation.
'''
new = '''### Bitstruct v1 semantic decision

Keep bitstructs simple: storage is restricted to `u8`, `u16`, `u32`, or `u64`, and declared field widths must exactly fill the storage width (unused bits are written explicitly as reserved fields). Fields are assigned in declaration order starting at least-significant bit 0. A 1-bit field has source type `bool`; wider fields use the smallest ordinary unsigned Forge integer type that can represent their width. Forge does not create arbitrary-width integer types such as `u3` or `u5`. Reads zero-extend into the ordinary field type. Compile-time-known out-of-range writes are errors and dynamic writes are checked rather than silently truncated. Bit numbering is defined on the numeric storage value; target memory endianness remains the ordinary representation of that storage integer.
'''
if old in text:
    text = text.replace(old, new, 1)
arch.write_text(text)

spec = Path("docs/forge-v1-spec.md")
text = spec.read_text()
old = '''`?` on optional values is not used for the same propagation syntax in v1; optional handling uses pattern matching or dedicated optional combinators, avoiding ambiguity between "not present" and "error".
'''
new = '''`?` on optional values is not used for the same propagation syntax in v1; optional handling uses pattern matching or dedicated optional combinators, avoiding ambiguity between "not present" and "error".

For `value?`, if `value` has type `Result[T, Ein]`, the enclosing function or closure must return `Result[R, Eout]` and `Ein` must be assignable to `Eout`. Forge v1 performs no Rust-style implicit `From` conversion for propagation. The expression itself has type `T`; an `Err` returns from the immediately enclosing function or closure.
'''
if old in text and "Forge v1 performs no Rust-style implicit `From` conversion" not in text:
    text = text.replace(old, new, 1)
needle = '''const PAGE_SIZE: usize = 4096;
```
'''
replacement = '''const PAGE_SIZE: usize = 4096;
const DOUBLE_PAGE: usize = PAGE_SIZE * 2;
```

Forge v1 constant evaluation is intentionally restricted. Constant expressions may use scalar literals, pure unary/binary operators, and references to other module `const` definitions. Constant dependencies may be forward references but cycles are rejected. Arbitrary function calls and general compile-time execution are not part of v1.
'''
if needle in text and "Constant dependencies may be forward references" not in text:
    text = text.replace(needle, replacement, 1)
spec.write_text(text)

print("final semantic cleanup patch applied")
