from pathlib import Path
import re


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing exact block: {label}")
    return text.replace(old, new, 1)


def regex_once(text: str, pattern: str, repl, label: str) -> str:
    out, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"regex {label}: expected 1 match, got {count}")
    return out

# ---------------------------------------------------------------------------
# AST/parser: context.<slot> is a first-class expression. Previously the
# language spec showed it, but Token::Context could not enter an expression.
# ---------------------------------------------------------------------------
ast = Path("crates/forge-frontend/src/ast_v1.rs")
text = ast.read_text()
text = replace_once(
    text,
    """    Keyword {\n        name: String,\n    },\n    Path {\n""",
    """    Keyword {\n        name: String,\n    },\n    Context {\n        name: String,\n    },\n    Path {\n""",
    "AST context expression",
)
ast.write_text(text)

parser = Path("crates/forge-frontend/src/parser_v1.rs")
text = parser.read_text()
text = replace_once(
    text,
    """    let path_expr = ident().map_with(|name, e| {\n""",
    """    let context_expr = just(Token::Context)\n        .ignore_then(just(Token::Dot))\n        .ignore_then(ident())\n        .map_with(|name, e| Node::new(ExprKind::Context { name }, span(e.span())));\n    let path_expr = ident().map_with(|name, e| {\n""",
    "parser context expr",
)
text = replace_once(
    text,
    """        keyword_expr,\n        array_expr,\n        path_expr,\n""",
    """        keyword_expr,\n        array_expr,\n        context_expr,\n        path_expr,\n""",
    "parser atom context",
)
parser.write_text(text)

# ---------------------------------------------------------------------------
# Body HIR / resolver: context slot is resolved syntax, not a fake identifier.
# ---------------------------------------------------------------------------
body = Path("crates/forge-frontend/src/body_hir_v1.rs")
text = body.read_text()
text = replace_once(
    text,
    """    Keyword {\n        name: String,\n    },\n    Name {\n""",
    """    Keyword {\n        name: String,\n    },\n    Context {\n        name: String,\n    },\n    Name {\n""",
    "HIR context expression",
)
text = replace_once(
    text,
    """            ExprKind::Keyword { name } => HirExprKind::Keyword { name: name.clone() },\n            ExprKind::Path { path } => HirExprKind::Name {\n""",
    """            ExprKind::Keyword { name } => HirExprKind::Keyword { name: name.clone() },\n            ExprKind::Context { name } => HirExprKind::Context { name: name.clone() },\n            ExprKind::Path { path } => HirExprKind::Name {\n""",
    "lower context expression",
)
body.write_text(text)

resolution = Path("crates/forge-frontend/src/resolution_v1.rs")
text = resolution.read_text()
# Context contains no lexical/module name requiring resolution.
text = replace_once(
    text,
    """            ExprKind::Path { path } if path.segments.len() == 1 => {\n""",
    """            ExprKind::Context { .. } => {}\n            ExprKind::Path { path } if path.segments.len() == 1 => {\n""",
    "resolver context expression",
)
resolution.write_text(text)

# ---------------------------------------------------------------------------
# Type layer: THIR-like semantic tables. FIR will consume these plans directly.
# ---------------------------------------------------------------------------
typecheck = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = typecheck.read_text()

# Concrete execution-context and duration types.
text = replace_once(
    text,
    """    Char,\n    Str,\n    Byte,\n""",
    """    Char,\n    Str,\n    Byte,\n    Duration,\n    ContextSlot(ContextSlot),\n""",
    "Ty context/duration",
)

public_semantics = r'''

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ContextSlot {
    Scratch,
    Logger,
    Clock,
    Random,
    Trace,
}

impl ContextSlot {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "scratch" => Some(Self::Scratch),
            "logger" => Some(Self::Logger),
            "clock" => Some(Self::Clock),
            "random" => Some(Self::Random),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    Value,
    SharedReference,
    MutableReference,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedCapture {
    pub local: LocalId,
    pub source: ResolvedName,
    pub ty: Ty,
    pub mode: CaptureMode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedClosurePlan {
    pub captures: Vec<TypedCapture>,
    pub params: Vec<(LocalId, Ty)>,
    pub result: Ty,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedPattern {
    pub span: Span,
    pub ty: Ty,
    pub kind: TypedPatternKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "pattern", rename_all = "snake_case")]
pub enum TypedPatternKind {
    Wildcard,
    Binding { local: LocalId },
    Literal { value: ast::PatternLiteral },
    Range {
        start: ast::PatternLiteral,
        end: ast::PatternLiteral,
        inclusive: bool,
    },
    Variant { name: String, fields: Vec<TypedPatternField> },
    None,
    Some { value: Box<TypedPattern> },
    Struct { fields: Vec<TypedPatternField> },
    Sequence { items: Vec<TypedPattern>, rest: Option<LocalId> },
    Or { patterns: Vec<TypedPattern> },
    As { local: LocalId, pattern: Box<TypedPattern> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedPatternField {
    pub name: String,
    pub ty: Ty,
    pub pattern: Option<Box<TypedPattern>>,
    pub shorthand_local: Option<LocalId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub patterns: Vec<TypedPattern>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedCallArgument {
    Provided { parameter: usize, argument: usize },
    Default { parameter: usize, value: HirExpr },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedCallPlan {
    pub target: DefId,
    pub method: bool,
    pub receiver: Option<ResolvedReceiver>,
    pub arguments: Vec<ResolvedCallArgument>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedSelectReceive {
    pub recv_target: DefId,
    pub payload_type: Ty,
}
'''
marker = "#[derive(Debug, Clone, PartialEq, Eq, Serialize)]\npub struct TypeDiagnostic"
if marker not in text:
    raise SystemExit("TypeDiagnostic marker missing")
text = text.replace(marker, public_semantics + "\n" + marker, 1)

# Semantic plan tables live on the body (ExprId is stable/body-local).
text = replace_once(
    text,
    """    pub expressions: Vec<TypedExpr>,\n    pub unsafe_expressions: BTreeSet<ExprId>,\n}\n""",
    """    pub expressions: Vec<TypedExpr>,\n    pub unsafe_expressions: BTreeSet<ExprId>,\n    pub call_plans: BTreeMap<ExprId, ResolvedCallPlan>,\n    pub closure_plans: BTreeMap<ExprId, TypedClosurePlan>,\n    pub match_plans: BTreeMap<ExprId, TypedMatchPlan>,\n    pub select_receives: BTreeMap<ExprId, TypedSelectReceive>,\n}\n""",
    "TypedBody semantic plan tables",
)

# Parameter signatures retain the lowered default expression and semantic local.
text = replace_once(
    text,
    """struct ParamSig {\n    name: String,\n    ty: Ty,\n    has_default: bool,\n}\n""",
    """struct ParamSig {\n    name: String,\n    ty: Ty,\n    has_default: bool,\n    local: Option<LocalId>,\n    default: Option<HirExpr>,\n}\n""",
    "ParamSig default HIR",
)

# The type environment needs bodies so defaults are associated with DefId/LocalId.
text = replace_once(
    text,
    """    let env = ModuleTypeEnv::build(source, module, &constant_values, &bitstructs);\n""",
    """    let env = ModuleTypeEnv::build(source, module, bodies, &constant_values, &bitstructs);\n""",
    "env build call bodies",
)
text = replace_once(
    text,
    """    fn build(\n        source: &ast::SourceFile,\n        module: &HirModule,\n        constants: &BTreeMap<DefId, ConstValue>,\n        bitstructs: &BTreeMap<DefId, BitStructInfo>,\n    ) -> Self {\n""",
    """    fn build(\n        source: &ast::SourceFile,\n        module: &HirModule,\n        bodies: &BodyHirOutput,\n        constants: &BTreeMap<DefId, ConstValue>,\n        bitstructs: &BTreeMap<DefId, BitStructInfo>,\n    ) -> Self {\n""",
    "env build signature bodies",
)

# Rewrite top-level function ParamSig construction.
old = '''                let params = function
                    .params
                    .iter()
                    .map(|p| ParamSig {
                        name: p.name.clone(),
                        ty: env.lower_ast_type(&p.ty, module),
                        has_default: p.default.is_some(),
                    })
                    .collect();
'''
new = '''                let body = bodies.functions.get(&id);
                let params = function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, p)| {
                        let local = body.and_then(|body| body.params.get(index).map(|(id, _)| *id));
                        let default = local.and_then(|local| {
                            body.and_then(|body| body.param_defaults.get(&local).cloned())
                        });
                        ParamSig {
                            name: p.name.clone(),
                            ty: env.lower_ast_type(&p.ty, module),
                            has_default: default.is_some(),
                            local,
                            default,
                        }
                    })
                    .collect();
'''
text = replace_once(text, old, new, "top-level ParamSig defaults")

# Rewrite method ParamSig construction.
old = '''                let params = method
                    .function
                    .params
                    .iter()
                    .map(|p| ParamSig {
                        name: p.name.clone(),
                        ty: env.lower_ast_type(&p.ty, module),
                        has_default: p.default.is_some(),
                    })
                    .collect::<Vec<_>>();
'''
new = '''                let body = bodies.functions.get(&method_def.id);
                let params = method
                    .function
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, p)| {
                        let local = body.and_then(|body| body.params.get(index).map(|(id, _)| *id));
                        let default = local.and_then(|local| {
                            body.and_then(|body| body.param_defaults.get(&local).cloned())
                        });
                        ParamSig {
                            name: p.name.clone(),
                            ty: env.lower_ast_type(&p.ty, module),
                            has_default: default.is_some(),
                            local,
                            default,
                        }
                    })
                    .collect::<Vec<_>>();
'''
text = replace_once(text, old, new, "method ParamSig defaults")

# Reduced method signatures clone all default information already.

# BodyChecker plan state.
text = replace_once(
    text,
    """    unsafe_expressions: BTreeSet<ExprId>,\n    diagnostics: &'d mut Vec<TypeDiagnostic>,\n""",
    """    unsafe_expressions: BTreeSet<ExprId>,\n    call_plans: BTreeMap<ExprId, ResolvedCallPlan>,\n    closure_plans: BTreeMap<ExprId, TypedClosurePlan>,\n    match_plans: BTreeMap<ExprId, TypedMatchPlan>,\n    select_receives: BTreeMap<ExprId, TypedSelectReceive>,\n    diagnostics: &'d mut Vec<TypeDiagnostic>,\n""",
    "BodyChecker plan fields",
)
text = replace_once(
    text,
    """            unsafe_expressions: BTreeSet::new(),\n            diagnostics,\n""",
    """            unsafe_expressions: BTreeSet::new(),\n            call_plans: BTreeMap::new(),\n            closure_plans: BTreeMap::new(),\n            match_plans: BTreeMap::new(),\n            select_receives: BTreeMap::new(),\n            diagnostics,\n""",
    "BodyChecker plan init",
)

# Persist plan tables in typed function bodies.
text = replace_once(
    text,
    """                unsafe_expressions: checker.unsafe_expressions,\n""",
    """                unsafe_expressions: checker.unsafe_expressions,\n                call_plans: checker.call_plans,\n                closure_plans: checker.closure_plans,\n                match_plans: checker.match_plans,\n                select_receives: checker.select_receives,\n""",
    "persist semantic plans",
)

# Globals use BodyChecker but don't expose function-body plans; defaults/captures in
# a global initializer are retained in its TypedExpr table and will get a dedicated
# global-init lowering path. The phase-1 TypedGlobal literal only has unsafe table.

# Extend ResolvedCallInfo with normalized arguments.
text = replace_once(
    text,
    """struct ResolvedCallInfo {\n    target: DefId,\n    method: bool,\n    receiver: Option<ResolvedReceiver>,\n    argument_parameters: Vec<usize>,\n}\n""",
    """struct ResolvedCallInfo {\n    target: DefId,\n    method: bool,\n    receiver: Option<ResolvedReceiver>,\n    argument_parameters: Vec<usize>,\n    arguments: Vec<ResolvedCallArgument>,\n}\n""",
    "ResolvedCallInfo normalized args",
)

# Function-arg checking now returns both the compatibility mapping retained for
# existing TypedExpr and the complete parameter-ordered call plan.
start = text.index("    fn check_function_args(\n")
end = text.index("\n    fn check_type_call(", start)
old_func = text[start:end]
new_func = r'''    fn check_function_args(
        &mut self,
        span: Span,
        sig: &FunctionSig,
        args: &[HirCallArg],
    ) -> (Vec<usize>, Vec<ResolvedCallArgument>) {
        let named = args.iter().any(|a| matches!(a, HirCallArg::Named { .. }));
        let mut provided = BTreeMap::<usize, usize>::new();
        let mut argument_parameters = Vec::with_capacity(args.len());
        if sig.named_arguments {
            if !args.is_empty() && !named {
                self.diagnostic(span, "call/named-only", "nfn calls require named arguments");
            }
            let mut seen = BTreeSet::new();
            for (argument, arg) in args.iter().enumerate() {
                let HirCallArg::Named { name, value } = arg else {
                    self.check_expr(arg_value(arg), None);
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
                    argument_parameters.push(parameter);
                    provided.insert(parameter, argument);
                    let actual = self.check_expr(value, Some(&param.ty));
                    self.require_assignable(value.span, &param.ty, &actual, "type/mismatch");
                } else {
                    self.diagnostic(
                        value.span,
                        "call/unknown-name",
                        format!("unknown named argument `{name}`"),
                    );
                    self.check_expr(value, None);
                }
            }
        } else {
            if named {
                self.diagnostic(
                    span,
                    "call/unknown-name",
                    "named arguments require an nfn declaration",
                );
            }
            for (argument, arg) in args.iter().enumerate() {
                let value = arg_value(arg);
                if let Some(param) = sig.params.get(argument) {
                    argument_parameters.push(argument);
                    provided.insert(argument, argument);
                    let actual = self.check_expr(value, Some(&param.ty));
                    self.require_assignable(value.span, &param.ty, &actual, "type/mismatch");
                } else {
                    self.diagnostic(value.span, "call/arity", "too many arguments");
                    self.check_expr(value, None);
                }
            }
        }

        let mut normalized = Vec::with_capacity(sig.params.len());
        for (parameter, param) in sig.params.iter().enumerate() {
            if let Some(argument) = provided.get(&parameter).copied() {
                normalized.push(ResolvedCallArgument::Provided { parameter, argument });
            } else if let Some(value) = &param.default {
                normalized.push(ResolvedCallArgument::Default {
                    parameter,
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
        (argument_parameters, normalized)
    }
'''
text = text[:start] + new_func + text[end:]

# Adjust check_call callers to unpack normalized call plans.
text = replace_once(
    text,
    """                let argument_parameters = self.check_function_args(span, &reduced, args);\n                return (\n                    sig.result,\n                    Some(ResolvedCallInfo {\n                        target: method_id,\n                        method: true,\n                        receiver,\n                        argument_parameters,\n                    }),\n                );\n""",
    """                let (argument_parameters, arguments) =\n                    self.check_function_args(span, &reduced, args);\n                return (\n                    sig.result,\n                    Some(ResolvedCallInfo {\n                        target: method_id,\n                        method: true,\n                        receiver,\n                        argument_parameters,\n                        arguments,\n                    }),\n                );\n""",
    "method normalized call",
)
text = replace_once(
    text,
    """                    let argument_parameters = self.check_function_args(span, &sig, args);\n                    return (\n                        sig.result,\n                        Some(ResolvedCallInfo {\n                            target: id,\n                            method: false,\n                            receiver: None,\n                            argument_parameters,\n                        }),\n                    );\n""",
    """                    let (argument_parameters, arguments) =\n                        self.check_function_args(span, &sig, args);\n                    return (\n                        sig.result,\n                        Some(ResolvedCallInfo {\n                            target: id,\n                            method: false,\n                            receiver: None,\n                            argument_parameters,\n                            arguments,\n                        }),\n                    );\n""",
    "function normalized call",
)

# Context overrides and select become semantically checked statements.
old = '''            HirStmtKind::WithContext { overrides, body } => {
                for (_, expr) in overrides {
                    self.check_expr(expr, None);
                }
                self.check_block(body);
            }
            HirStmtKind::Select { arms } => {
                for arm in arms {
                    match arm {
                        crate::body_hir::HirSelectArm::Receive {
                            channel,
                            pattern,
                            body,
                        } => {
                            self.check_expr(channel, None);
                            self.check_pattern(pattern, &Ty::Unknown);
                            self.check_block(body);
                        }
                        crate::body_hir::HirSelectArm::Timeout { duration, body } => {
                            self.check_expr(duration, None);
                            self.check_block(body);
                        }
                    }
                }
            }
'''
new = '''            HirStmtKind::WithContext { overrides, body } => {
                let mut seen = BTreeSet::new();
                for (name, expr) in overrides {
                    if ContextSlot::from_name(name).is_none() {
                        self.diagnostic(
                            expr.span,
                            "context/unknown-slot",
                            format!("unknown core context slot `{name}`"),
                        );
                    }
                    if !seen.insert(name.clone()) {
                        self.diagnostic(
                            expr.span,
                            "context/duplicate-slot",
                            format!("context slot `{name}` is overridden more than once"),
                        );
                    }
                    let actual = self.check_expr(expr, None);
                    if matches!(actual, Ty::Unknown | Ty::Error | Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral) {
                        self.diagnostic(
                            expr.span,
                            "context/value-type",
                            "context override requires a concrete typed value",
                        );
                    }
                }
                self.check_block(body);
            }
            HirStmtKind::Select { arms } => {
                let mut timeout_seen = false;
                for arm in arms {
                    match arm {
                        crate::body_hir::HirSelectArm::Receive {
                            channel,
                            pattern,
                            body,
                        } => {
                            let channel_ty = self.check_expr(channel, None);
                            if let Some((recv_target, sig)) = self.env.lookup_method(&channel_ty, "recv") {
                                let sig = sig.clone();
                                if sig.params.len() != 1 {
                                    self.diagnostic(
                                        channel.span,
                                        "select/channel-protocol",
                                        "select channel `recv` method must take only its receiver",
                                    );
                                    self.check_pattern(pattern, &Ty::Error);
                                } else {
                                    let payload_type = sig.result.clone();
                                    if !self.pattern_is_irrefutable(pattern, &payload_type) {
                                        self.diagnostic(
                                            pattern.span,
                                            "select/refutable-pattern",
                                            "select receive bindings must be irrefutable in Forge v1",
                                        );
                                    }
                                    self.check_pattern(pattern, &payload_type);
                                    self.select_receives.insert(
                                        channel.id,
                                        TypedSelectReceive {
                                            recv_target,
                                            payload_type,
                                        },
                                    );
                                }
                            } else {
                                self.diagnostic(
                                    channel.span,
                                    "select/channel-protocol",
                                    format!(
                                        "type {channel_ty:?} does not provide the required `recv` method"
                                    ),
                                );
                                self.check_pattern(pattern, &Ty::Error);
                            }
                            self.check_block(body);
                        }
                        crate::body_hir::HirSelectArm::Timeout { duration, body } => {
                            if timeout_seen {
                                self.diagnostic(
                                    duration.span,
                                    "select/duplicate-timeout",
                                    "select may contain at most one timeout arm",
                                );
                            }
                            timeout_seen = true;
                            let actual = self.check_expr(duration, Some(&Ty::Duration));
                            self.require_assignable(
                                duration.span,
                                &Ty::Duration,
                                &actual,
                                "select/timeout-type",
                            );
                            self.check_block(body);
                        }
                    }
                }
            }
'''
text = replace_once(text, old, new, "context/select semantic checking")

# Reader duration and context slot expression types.
text = replace_once(
    text,
    """            HirExprKind::Keyword { .. } | HirExprKind::ReaderForm { .. } => Ty::Unknown,\n            HirExprKind::Name { reference } => self.type_of_name(reference.root),\n""",
    """            HirExprKind::Keyword { .. } => Ty::Unknown,\n            HirExprKind::ReaderForm { tag, .. } if tag == \"duration\" => Ty::Duration,\n            HirExprKind::ReaderForm { .. } => Ty::Unknown,\n            HirExprKind::Context { name } => match ContextSlot::from_name(name) {\n                Some(slot) => Ty::ContextSlot(slot),\n                None => {\n                    self.diagnostic(\n                        expr.span,\n                        \"context/unknown-slot\",\n                        format!(\"unknown core context slot `{name}`\"),\n                    );\n                    Ty::Error\n                }\n            },\n            HirExprKind::Name { reference } => self.type_of_name(reference.root),\n""",
    "context/duration expression typing",
)

# Replace closure checking with explicit capture plan. Captured names keep their
# source type; capture mode controls environment representation (transparent
# lexical semantics, like Rust's capture projections rather than changing the
# source variable's type).
pattern = r'''            HirExprKind::Closure \{\n                params,\n                return_type,\n                body,\n                \.\.,\n            \} => \{.*?\n            \}\n            HirExprKind::Match \{ value, arms \} => \{'''
replacement = r'''            HirExprKind::Closure {
                captures,
                params,
                return_type,
                body,
            } => {
                let outer_types = self.local_types.clone();
                let mut typed_captures = Vec::with_capacity(captures.len());
                for capture in captures {
                    let source_ty = match capture.source {
                        ResolvedName::Local(id) => outer_types.get(&id).cloned().unwrap_or(Ty::Error),
                        other => self.type_of_name(other),
                    };
                    let mode = if capture.by_reference {
                        if capture.mutable {
                            CaptureMode::MutableReference
                        } else {
                            CaptureMode::SharedReference
                        }
                    } else {
                        CaptureMode::Value
                    };
                    if mode == CaptureMode::MutableReference {
                        if let ResolvedName::Local(id) = capture.source {
                            if !self.mutable_locals.contains(&id) {
                                self.diagnostic(
                                    expr.span,
                                    "closure/mutable-capture",
                                    "mutable-reference capture requires a mutable source binding",
                                );
                            }
                        }
                    }
                    typed_captures.push(TypedCapture {
                        local: capture.local,
                        source: capture.source,
                        ty: source_ty,
                        mode,
                    });
                }

                let saved = std::mem::take(&mut self.local_types);
                for capture in &typed_captures {
                    self.local_types.insert(capture.local, capture.ty.clone());
                }
                let ptys: Vec<(LocalId, Ty)> = params
                    .iter()
                    .map(|(id, t)| {
                        let ty = self
                            .env
                            .lower_hir_type_with_locals(t, &self.local_constants);
                        self.local_types.insert(*id, ty.clone());
                        (*id, ty)
                    })
                    .collect();
                let result = return_type
                    .as_ref()
                    .map(|t| {
                        self.env
                            .lower_hir_type_with_locals(t, &self.local_constants)
                    })
                    .unwrap_or_else(|| {
                        self.diagnostic(
                            expr.span,
                            "closure/return-type-required",
                            "Forge v1 closures require an explicit return type",
                        );
                        Ty::Error
                    });
                let old_return = std::mem::replace(&mut self.expected_return, result.clone());
                self.check_block(body);
                self.expected_return = old_return;
                self.local_types.extend(saved);
                self.closure_plans.insert(
                    expr.id,
                    TypedClosurePlan {
                        captures: typed_captures,
                        params: ptys.clone(),
                        result: result.clone(),
                    },
                );
                Ty::Closure {
                    params: ptys.into_iter().map(|(_, ty)| ty).collect(),
                    result: Box::new(result),
                }
            }
            HirExprKind::Match { value, arms } => {'''
text = regex_once(text, pattern, replacement, "closure semantic plan")

# Match: retain fully typed structural patterns after normal semantic checking.
text = replace_once(
    text,
    """                self.check_match_exhaustiveness(expr.span, &matched, arms);\n                result\n""",
    """                self.check_match_exhaustiveness(expr.span, &matched, arms);\n                let patterns = arms\n                    .iter()\n                    .map(|arm| self.resolve_typed_pattern(&arm.pattern, &matched))\n                    .collect();\n                self.match_plans.insert(\n                    expr.id,\n                    TypedMatchPlan {\n                        scrutinee_type: matched.clone(),\n                        patterns,\n                    },\n                );\n                result\n""",
    "match semantic plan",
)

# Store normalized call plan before ResolvedCallInfo is consumed into TypedExprKind.
old = '''        let base_kind = if let Some((source_error, target_error)) = resolved_try {
'''
new = '''        if let Some(call) = resolved_call.as_ref() {
            self.call_plans.insert(
                expr.id,
                ResolvedCallPlan {
                    target: call.target,
                    method: call.method,
                    receiver: call.receiver,
                    arguments: call.arguments.clone(),
                },
            );
        }
        let base_kind = if let Some((source_error, target_error)) = resolved_try {
'''
text = replace_once(text, old, new, "store normalized call plan")

# Non-finite matches must still have an unguarded irrefutable arm. This mirrors
# Rust's exhaustive match rule and avoids a hidden FIR fallthrough/UB path.
text = replace_once(
    text,
    """        let Some(required) = self.finite_match_cases(ty) else {\n            return;\n        };\n""",
    """        let Some(required) = self.finite_match_cases(ty) else {\n            if !arms.iter().any(|arm| {\n                arm.guard.is_none() && self.pattern_is_irrefutable(&arm.pattern, ty)\n            }) {\n                self.diagnostic(\n                    span,\n                    \"match/non-exhaustive\",\n                    \"non-exhaustive match over an open domain; add an unguarded wildcard/irrefutable arm\",\n                );\n            }\n            return;\n        };\n""",
    "open-domain match exhaustiveness",
)

# Typed-pattern resolver inserted before match exhaustiveness.
marker = "    fn check_match_exhaustiveness(\n"
if marker not in text:
    raise SystemExit("match exhaustiveness marker missing")
resolver = r'''    fn resolve_typed_pattern(&mut self, pattern: &HirPattern, ty: &Ty) -> TypedPattern {
        let kind = match &pattern.kind {
            HirPatternKind::Wildcard => TypedPatternKind::Wildcard,
            HirPatternKind::Binding { local, .. } => TypedPatternKind::Binding { local: *local },
            HirPatternKind::Literal { value } => TypedPatternKind::Literal {
                value: value.clone(),
            },
            HirPatternKind::Range { start, end, inclusive } => TypedPatternKind::Range {
                start: start.clone(),
                end: end.clone(),
                inclusive: *inclusive,
            },
            HirPatternKind::None { .. } => TypedPatternKind::None,
            HirPatternKind::Some { value } => {
                let inner = match ty {
                    Ty::Optional { inner } => inner.as_ref().clone(),
                    _ => Ty::Error,
                };
                TypedPatternKind::Some {
                    value: Box::new(self.resolve_typed_pattern(value, &inner)),
                }
            }
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                let defs = match &expected {
                    Ty::Nominal(id) => match self.env.types.get(id).map(|info| &info.kind) {
                        Some(TypeInfoKind::Struct(fields)) => Some(fields.clone()),
                        _ => None,
                    },
                    _ => None,
                };
                TypedPatternKind::Struct {
                    fields: self.resolve_typed_pattern_fields(fields, defs.as_ref()),
                }
            }
            HirPatternKind::Variant { namespace, name, fields, .. } => {
                let expected = self.env.ty_from_ref(namespace);
                let defs = match &expected {
                    Ty::Nominal(id) => match self.env.types.get(id).map(|info| &info.kind) {
                        Some(TypeInfoKind::Tagged(variants)) => variants.get(name).cloned(),
                        _ => None,
                    },
                    _ => None,
                };
                TypedPatternKind::Variant {
                    name: name.clone(),
                    fields: self.resolve_typed_pattern_fields(fields, defs.as_ref()),
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let element = match ty {
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => element.as_ref().clone(),
                    _ => Ty::Error,
                };
                TypedPatternKind::Sequence {
                    items: items
                        .iter()
                        .map(|item| self.resolve_typed_pattern(item, &element))
                        .collect(),
                    rest: *rest,
                }
            }
            HirPatternKind::Or { patterns } => TypedPatternKind::Or {
                patterns: patterns
                    .iter()
                    .map(|pattern| self.resolve_typed_pattern(pattern, ty))
                    .collect(),
            },
            HirPatternKind::As { local, pattern } => TypedPatternKind::As {
                local: *local,
                pattern: Box::new(self.resolve_typed_pattern(pattern, ty)),
            },
            HirPatternKind::Map { .. } => TypedPatternKind::Wildcard,
        };
        TypedPattern {
            span: pattern.span,
            ty: ty.clone(),
            kind,
        }
    }

    fn resolve_typed_pattern_fields(
        &mut self,
        fields: &[crate::body_hir::HirPatternField],
        defs: Option<&BTreeMap<String, FieldInfo>>,
    ) -> Vec<TypedPatternField> {
        fields
            .iter()
            .map(|field| {
                let ty = defs
                    .and_then(|defs| defs.get(&field.name))
                    .map(|field| field.ty.clone())
                    .or_else(|| {
                        field
                            .shorthand_local
                            .and_then(|local| self.local_types.get(&local).cloned())
                    })
                    .unwrap_or(Ty::Error);
                TypedPatternField {
                    name: field.name.clone(),
                    ty: ty.clone(),
                    pattern: field
                        .pattern
                        .as_ref()
                        .map(|pattern| Box::new(self.resolve_typed_pattern(pattern, &ty))),
                    shorthand_local: field.shorthand_local,
                }
            })
            .collect()
    }

'''
text = text.replace(marker, resolver + marker, 1)

# Compile-time-known bitstruct writes must fit the field width. Dynamic values
# are checked in FIR/runtime in phase 3.
text = replace_once(
    text,
    """                self.require_assignable(value.span, &target_ty, &value_ty, \"type/mismatch\");\n            }\n""",
    """                self.require_assignable(value.span, &target_ty, &value_ty, \"type/mismatch\");\n                self.check_bitstruct_write(target, value);\n            }\n""",
    "bitstruct assignment range hook",
)
marker = "    fn check_assignment_target(&mut self, target: &HirExpr) {\n"
helper = r'''    fn check_bitstruct_write(&mut self, target: &HirExpr, value: &HirExpr) {
        let HirExprKind::Member { base, name } = &target.kind else {
            return;
        };
        let base_ty = self.place_type(base);
        let nominal = match base_ty {
            Ty::Nominal(id) => Some(id),
            Ty::Reference { inner, .. } => match *inner {
                Ty::Nominal(id) => Some(id),
                _ => None,
            },
            _ => None,
        };
        let Some(id) = nominal else { return; };
        let Some(TypeInfoKind::BitStruct(info)) = self.env.types.get(&id).map(|info| &info.kind) else {
            return;
        };
        let Some(field) = info.fields.get(name) else { return; };
        if field.width == 1 {
            return;
        }
        if let Ok(ConstValue::Integer { value: integer }) = eval_const_hir_with_locals(
            value,
            &self.env.constants,
            &self.local_constants,
        ) {
            let limit = 1i128.checked_shl(field.width).unwrap_or(i128::MAX);
            if integer < 0 || integer >= limit {
                self.diagnostic(
                    value.span,
                    "bitstruct/value-range",
                    format!(
                        "value {integer} does not fit the {width}-bit field `{name}`",
                        width = field.width
                    ),
                );
            }
        }
    }

'''
if marker not in text:
    raise SystemExit("assignment marker missing")
text = text.replace(marker, helper + marker, 1)

# Duration is concrete and context slots are opaque-but-concrete semantic values.
# No special is_assignable case needed beyond equality.

typecheck.write_text(text)

# ---------------------------------------------------------------------------
# FIR compile bridge for the new HIR Context expression. Real context scope,
# match, closure, select, defaults and globals are phase 3.
# ---------------------------------------------------------------------------
fir = Path("crates/forge-frontend/src/fir_v1.rs")
text = fir.read_text()
text = replace_once(
    text,
    """    LoadGlobal {\n        global: DefId,\n    },\n""",
    """    LoadGlobal {\n        global: DefId,\n    },\n    ContextGet {\n        slot: crate::typecheck::ContextSlot,\n    },\n""",
    "FIR ContextGet",
)
text = replace_once(
    text,
    """            HirExprKind::None => self.emit_value(expr.span, ty, FirInstructionKind::MakeNone),\n            HirExprKind::Name { reference } => self.lower_name(expr.span, reference.root, ty),\n""",
    """            HirExprKind::None => self.emit_value(expr.span, ty, FirInstructionKind::MakeNone),\n            HirExprKind::Context { name } => {\n                let Some(slot) = crate::typecheck::ContextSlot::from_name(name) else {\n                    return self.poison(expr.span, Ty::Error);\n                };\n                self.emit_value(expr.span, ty, FirInstructionKind::ContextGet { slot })\n            }\n            HirExprKind::Name { reference } => self.lower_name(expr.span, reference.root, ty),\n""",
    "lower ContextGet",
)
# ContextSlot::from_name is currently private; make it crate-visible.
typecheck_text = typecheck.read_text().replace("    fn from_name(name: &str) -> Option<Self> {", "    pub(crate) fn from_name(name: &str) -> Option<Self> {", 1)
typecheck.write_text(typecheck_text)
fir.write_text(text)

# ---------------------------------------------------------------------------
# Tests for semantic normalization.
# ---------------------------------------------------------------------------
tests = Path("crates/forge-frontend/tests/typecheck.rs")
t = tests.read_text()
if "normalizes_named_defaults_before_fir" not in t:
    t += r'''

#[test]
fn normalizes_named_defaults_before_fir() {
    let output = check(
        r#"
        module test.default_plan;
        nfn connect(host: str, port: u16 = 443u16) -> bool { return true; }
        fn main() -> bool { return connect(:host = "example"); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(output.functions.values().any(|body| body.call_plans.values().any(|plan| {
        plan.arguments.iter().any(|arg| matches!(
            arg,
            forge_frontend::ResolvedCallArgument::Default { parameter: 1, .. }
        ))
    })));
}

#[test]
fn retains_typed_match_plan() {
    let output = check(
        r#"
        module test.match_plan;
        tagged Value { Left { x: u32; }, Right { x: u32; }, }
        fn read(value: Value) -> u32 {
            return match (value) {
                Value::Left{x} => x,
                Value::Right{x} => x,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let plan = output
        .functions
        .values()
        .flat_map(|body| body.match_plans.values())
        .next()
        .expect("match plan");
    assert_eq!(plan.patterns.len(), 2);
    assert!(matches!(plan.patterns[0].kind, forge_frontend::TypedPatternKind::Variant { .. }));
}

#[test]
fn open_domain_match_requires_irrefutable_arm() {
    let output = check(
        r#"
        module test.open_match;
        fn read(value: u32) -> u32 {
            return match (value) { 1u32 => 1u32 };
        }
        "#,
    );
    assert!(has(&output, "match/non-exhaustive"), "{:?}", output.diagnostics);
}

#[test]
fn closure_capture_plan_keeps_source_type_and_mode() {
    let output = check(
        r#"
        module test.closure_plan;
        fn main() -> u32 {
            var count: u32 = 1u32;
            val f = [&mut count]() -> u32 { return count; };
            return 0u32;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let plan = output
        .functions
        .values()
        .flat_map(|body| body.closure_plans.values())
        .next()
        .expect("closure plan");
    assert_eq!(plan.captures.len(), 1);
    assert_eq!(plan.captures[0].mode, forge_frontend::CaptureMode::MutableReference);
    assert_eq!(
        plan.captures[0].ty,
        Ty::Int { signed: false, width: IntWidth::W32 }
    );
}

#[test]
fn context_slots_are_concrete_and_validated() {
    let good = check(
        r#"
        module test.context_good;
        fn main(value: u32) -> u32 {
            val slot = context.logger;
            with context (:logger = value) { return value; }
        }
        "#,
    );
    assert!(good.diagnostics.is_empty(), "{:?}", good.diagnostics);

    let bad = check(
        r#"
        module test.context_bad;
        fn main(value: u32) -> u32 {
            with context (:database = value) { return value; }
        }
        "#,
    );
    assert!(has(&bad, "context/unknown-slot"), "{:?}", bad.diagnostics);
}

#[test]
fn select_resolves_nominal_recv_protocol() {
    let output = check(
        r#"
        module test.select_protocol;
        struct Jobs { marker: u32; }
        impl Jobs {
            fn recv(self: &Jobs) -> u32 { return self.marker; }
        }
        fn consume(jobs: Jobs) -> void {
            select {
                recv jobs -> job => { val x: u32 = job; }
                timeout #duration "100ms" => { }
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(output.functions.values().any(|body| !body.select_receives.is_empty()));
}

#[test]
fn bitstruct_constant_write_is_range_checked() {
    let output = check(
        r#"
        module test.bitstruct_write;
        bitstruct Status: u8 { mode: 3; reserved: 5; }
        fn set(value: &mut Status) -> void { value.mode = 8u8; }
        "#,
    );
    assert!(has(&output, "bitstruct/value-range"), "{:?}", output.diagnostics);
}
'''
    tests.write_text(t)

print("semantic normalization phase 2 applied")
