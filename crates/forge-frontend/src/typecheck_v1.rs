use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{self, BinaryOp, DeclKind, Span, UnaryOp},
    body_hir::{
        BodyHirOutput, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern, HirPatternKind,
        HirStmt, HirStmtKind, HirType, HirTypeKind, HirTypeRef,
    },
    hir::{DefId, HirModule},
    resolution::{LocalId, ResolvedName},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum IntWidth {
    W8,
    W16,
    W32,
    W64,
    Pointer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Ty {
    Error,
    Unknown,
    Never,
    Void,
    Bool,
    Char,
    Str,
    Byte,
    Int {
        signed: bool,
        width: IntWidth,
    },
    Float {
        bits: u8,
    },
    IntLiteral,
    FloatLiteral,
    NoneLiteral,
    Nominal(DefId),
    Pointer {
        volatile: bool,
        inner: Box<Ty>,
    },
    Reference {
        mutable: bool,
        inner: Box<Ty>,
    },
    Optional {
        inner: Box<Ty>,
    },
    Slice {
        mutable: bool,
        element: Box<Ty>,
    },
    Array {
        element: Box<Ty>,
    },
    Result {
        ok: Box<Ty>,
        error: Box<Ty>,
    },
    Function {
        params: Vec<Ty>,
        result: Box<Ty>,
        named_arguments: bool,
    },
    Closure {
        params: Vec<Ty>,
        result: Box<Ty>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDiagnostic {
    pub span: Span,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedExpr {
    pub span: Span,
    pub ty: Ty,
    pub kind: TypedExprKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum TypedExprKind {
    Source { hir: HirExpr },
    OptionalPromote { value: Box<TypedExpr> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedBody {
    pub owner: DefId,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub expressions: Vec<TypedExpr>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct TypeCheckOutput {
    pub functions: BTreeMap<DefId, TypedBody>,
    pub global_types: BTreeMap<DefId, Ty>,
    pub diagnostics: Vec<TypeDiagnostic>,
}

#[derive(Debug, Clone)]
struct FunctionSig {
    params: Vec<ParamSig>,
    result: Ty,
    named_arguments: bool,
}

#[derive(Debug, Clone)]
struct ParamSig {
    name: String,
    ty: Ty,
    has_default: bool,
}

#[derive(Debug, Clone)]
struct TypeInfo {
    kind: TypeInfoKind,
}

#[derive(Debug, Clone)]
enum TypeInfoKind {
    Distinct(Ty),
    Alias(Ty),
    Nominal,
}

pub fn type_check_module(
    source: &ast::SourceFile,
    module: &HirModule,
    bodies: &BodyHirOutput,
) -> TypeCheckOutput {
    let mut output = TypeCheckOutput::default();
    let env = ModuleTypeEnv::build(source, module);

    for (owner, body) in &bodies.functions {
        let expected_return = env
            .functions
            .get(owner)
            .map(|sig| sig.result.clone())
            .unwrap_or(Ty::Unknown);
        let mut checker = BodyChecker::new(&env, expected_return, &mut output.diagnostics);
        for (local, ty) in &body.params {
            checker.local_types.insert(*local, env.lower_hir_type(ty));
        }
        checker.check_block(&body.block);
        output.functions.insert(
            *owner,
            TypedBody {
                owner: *owner,
                local_types: checker.local_types,
                expressions: checker.expressions,
            },
        );
    }

    for (owner, global) in &bodies.globals {
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

    output
}

struct ModuleTypeEnv {
    types: BTreeMap<DefId, TypeInfo>,
    functions: BTreeMap<DefId, FunctionSig>,
}

impl ModuleTypeEnv {
    fn build(source: &ast::SourceFile, module: &HirModule) -> Self {
        let mut env = Self {
            types: BTreeMap::new(),
            functions: BTreeMap::new(),
        };

        // Establish all nominal identities first so aliases/signatures can refer forward.
        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            match &declaration.kind.kind {
                DeclKind::Struct(_)
                | DeclKind::Enum(_)
                | DeclKind::Tagged(_)
                | DeclKind::BitStruct(_) => {
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Nominal,
                        },
                    );
                }
                DeclKind::Distinct(_) | DeclKind::TypeAlias(_) => {
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Nominal,
                        },
                    );
                }
                _ => {}
            }
        }

        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            match &declaration.kind.kind {
                DeclKind::Distinct(value) => {
                    let ty = env.lower_ast_type(&value.underlying, module);
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Distinct(ty),
                        },
                    );
                }
                DeclKind::TypeAlias(value) => {
                    let ty = env.lower_ast_type(&value.target, module);
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Alias(ty),
                        },
                    );
                }
                _ => {}
            }
        }

        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            if let DeclKind::Function(function) = &declaration.kind.kind {
                let params = function
                    .params
                    .iter()
                    .map(|p| ParamSig {
                        name: p.name.clone(),
                        ty: env.lower_ast_type(&p.ty, module),
                        has_default: p.default.is_some(),
                    })
                    .collect();
                let result = function
                    .return_type
                    .as_ref()
                    .map(|t| env.lower_ast_type(t, module))
                    .unwrap_or(Ty::Void);
                env.functions.insert(
                    id,
                    FunctionSig {
                        params,
                        result,
                        named_arguments: function.named_arguments,
                    },
                );
            }
        }
        env
    }

    fn is_distinct_type(&self, ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::Nominal(id)
                if matches!(self.types.get(id).map(|info| &info.kind), Some(TypeInfoKind::Distinct(_)))
        )
    }

    fn lower_ast_type(&self, ty: &ast::TypeNode, module: &HirModule) -> Ty {
        match &ty.kind {
            ast::TypeKind::Named { path } if path.segments.len() == 1 => {
                let name = &path.segments[0];
                if let Some(ty) = builtin_ty(name) {
                    return ty;
                }
                if let Some(id) = module.symbols.get(name).and_then(|s| s.type_def) {
                    return self.resolve_type_def(id);
                }
                Ty::Unknown
            }
            ast::TypeKind::Named { .. } => Ty::Unknown,
            ast::TypeKind::Pointer { volatile, inner } => Ty::Pointer {
                volatile: *volatile,
                inner: Box::new(self.lower_ast_type(inner, module)),
            },
            ast::TypeKind::Reference { mutable, inner } => Ty::Reference {
                mutable: *mutable,
                inner: Box::new(self.lower_ast_type(inner, module)),
            },
            ast::TypeKind::Optional { inner } => Ty::Optional {
                inner: Box::new(self.lower_ast_type(inner, module)),
            },
            ast::TypeKind::Slice { mutable, element } => Ty::Slice {
                mutable: *mutable,
                element: Box::new(self.lower_ast_type(element, module)),
            },
            ast::TypeKind::Array { element, .. } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
            },
            ast::TypeKind::Result { ok, error } => Ty::Result {
                ok: Box::new(self.lower_ast_type(ok, module)),
                error: Box::new(self.lower_ast_type(error, module)),
            },
            ast::TypeKind::Function { params, result } => Ty::Function {
                params: params
                    .iter()
                    .map(|p| self.lower_ast_type(p, module))
                    .collect(),
                result: Box::new(self.lower_ast_type(result, module)),
                named_arguments: false,
            },
            ast::TypeKind::Closure { params, result } => Ty::Closure {
                params: params
                    .iter()
                    .map(|p| self.lower_ast_type(p, module))
                    .collect(),
                result: Box::new(self.lower_ast_type(result, module)),
            },
            ast::TypeKind::Annotated { inner, .. } => self.lower_ast_type(inner, module),
        }
    }

    fn lower_hir_type(&self, ty: &HirType) -> Ty {
        match &ty.kind {
            HirTypeKind::Named { reference } => self.ty_from_ref(reference),
            HirTypeKind::Pointer { volatile, inner } => Ty::Pointer {
                volatile: *volatile,
                inner: Box::new(self.lower_hir_type(inner)),
            },
            HirTypeKind::Reference { mutable, inner } => Ty::Reference {
                mutable: *mutable,
                inner: Box::new(self.lower_hir_type(inner)),
            },
            HirTypeKind::Optional { inner } => Ty::Optional {
                inner: Box::new(self.lower_hir_type(inner)),
            },
            HirTypeKind::Slice { mutable, element } => Ty::Slice {
                mutable: *mutable,
                element: Box::new(self.lower_hir_type(element)),
            },
            HirTypeKind::Array { element, .. } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
            },
            HirTypeKind::Result { ok, error } => Ty::Result {
                ok: Box::new(self.lower_hir_type(ok)),
                error: Box::new(self.lower_hir_type(error)),
            },
            HirTypeKind::Function { params, result } => Ty::Function {
                params: params.iter().map(|p| self.lower_hir_type(p)).collect(),
                result: Box::new(self.lower_hir_type(result)),
                named_arguments: false,
            },
            HirTypeKind::Closure { params, result } => Ty::Closure {
                params: params.iter().map(|p| self.lower_hir_type(p)).collect(),
                result: Box::new(self.lower_hir_type(result)),
            },
            HirTypeKind::Annotated { inner, .. } => self.lower_hir_type(inner),
        }
    }

    fn ty_from_ref(&self, reference: &HirTypeRef) -> Ty {
        match reference {
            HirTypeRef::Builtin { name } => builtin_ty(name).unwrap_or(Ty::Error),
            HirTypeRef::Def(id) => self.resolve_type_def(*id),
            HirTypeRef::Import { .. } => Ty::Unknown,
            HirTypeRef::Error => Ty::Error,
        }
    }

    fn resolve_type_def(&self, id: DefId) -> Ty {
        match self.types.get(&id).map(|i| &i.kind) {
            Some(TypeInfoKind::Alias(ty)) => ty.clone(),
            Some(TypeInfoKind::Distinct(_)) | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),
            None => Ty::Unknown,
        }
    }

    fn distinct_underlying(&self, id: DefId) -> Option<&Ty> {
        match self.types.get(&id).map(|i| &i.kind) {
            Some(TypeInfoKind::Distinct(ty)) => Some(ty),
            _ => None,
        }
    }
}

struct BodyChecker<'a, 'd> {
    env: &'a ModuleTypeEnv,
    expected_return: Ty,
    local_types: BTreeMap<LocalId, Ty>,
    mutable_locals: BTreeSet<LocalId>,
    expressions: Vec<TypedExpr>,
    diagnostics: &'d mut Vec<TypeDiagnostic>,
}

impl<'a, 'd> BodyChecker<'a, 'd> {
    fn new(
        env: &'a ModuleTypeEnv,
        expected_return: Ty,
        diagnostics: &'d mut Vec<TypeDiagnostic>,
    ) -> Self {
        Self {
            env,
            expected_return,
            local_types: BTreeMap::new(),
            mutable_locals: BTreeSet::new(),
            expressions: Vec::new(),
            diagnostics,
        }
    }

    fn diagnostic(&mut self, span: Span, code: &str, message: impl Into<String>) {
        self.diagnostics.push(TypeDiagnostic {
            span,
            code: code.into(),
            message: message.into(),
        });
    }

    fn check_block(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
    }

    fn check_stmt(&mut self, stmt: &HirStmt) {
        match &stmt.kind {
            HirStmtKind::Value {
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
                self.bind_pattern_type(pattern, &final_ty);
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
            }
            HirStmtKind::Assignment { target, value } => {
                self.check_assignment_target(target);
                let target_ty = self.check_expr(target, None);
                let value_ty = self.check_expr(value, Some(&target_ty));
                self.require_assignable(value.span, &target_ty, &value_ty, "type/mismatch");
            }
            HirStmtKind::Expr { expr } => {
                self.check_expr(expr, None);
            }
            HirStmtKind::Return { tail, value } => {
                if *tail
                    && !matches!(
                        value.as_ref().map(|e| &e.kind),
                        Some(HirExprKind::Call { .. })
                    )
                {
                    self.diagnostic(
                        stmt.span,
                        "control/tail-call-required",
                        "`return tail` requires a function call",
                    );
                }
                let actual = value
                    .as_ref()
                    .map(|e| self.check_expr(e, Some(&self.expected_return.clone())))
                    .unwrap_or(Ty::Void);
                if !self.is_assignable(&self.expected_return, &actual) {
                    self.diagnostic(
                        stmt.span,
                        "type/return",
                        format!(
                            "return type mismatch: expected {:?}, found {:?}",
                            self.expected_return, actual
                        ),
                    );
                }
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                let c = self.check_expr(condition, Some(&Ty::Bool));
                self.require_assignable(condition.span, &Ty::Bool, &c, "type/mismatch");
                self.check_block(then_block);
                if let Some(branch) = else_branch {
                    self.check_stmt(branch);
                }
            }
            HirStmtKind::While { condition, body } => {
                let c = self.check_expr(condition, Some(&Ty::Bool));
                self.require_assignable(condition.span, &Ty::Bool, &c, "type/mismatch");
                self.check_block(body);
            }
            HirStmtKind::ForC {
                init,
                condition,
                step,
                body,
            } => {
                if let Some(init) = init {
                    self.check_stmt(init);
                }
                if let Some(condition) = condition {
                    let c = self.check_expr(condition, Some(&Ty::Bool));
                    self.require_assignable(condition.span, &Ty::Bool, &c, "type/mismatch");
                }
                if let Some(step) = step {
                    self.check_stmt(step);
                }
                self.check_block(body);
            }
            HirStmtKind::ForEach {
                mutable,
                pattern,
                iterable,
                body,
            } => {
                let iter_ty = self.check_expr(iterable, None);
                let element = match iter_ty {
                    Ty::Array { element } | Ty::Slice { element, .. } => *element,
                    _ => Ty::Unknown,
                };
                self.bind_pattern_type(pattern, &element);
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
                self.check_block(body);
            }
            HirStmtKind::DeferExpr { expr } => {
                self.check_expr(expr, None);
            }
            HirStmtKind::DeferBlock { block }
            | HirStmtKind::Unsafe { block }
            | HirStmtKind::Block { block } => self.check_block(block),
            HirStmtKind::WithContext { overrides, body } => {
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
                            self.bind_pattern_type(pattern, &Ty::Unknown);
                            self.check_block(body);
                        }
                        crate::body_hir::HirSelectArm::Timeout { duration, body } => {
                            self.check_expr(duration, None);
                            self.check_block(body);
                        }
                    }
                }
            }
            HirStmtKind::Break | HirStmtKind::Continue => {}
        }
    }

    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut ty = match &expr.kind {
            HirExprKind::Integer { text } => integer_literal_ty(text),
            HirExprKind::Float { text } => float_literal_ty(text),
            HirExprKind::Character { .. } => Ty::Char,
            HirExprKind::String { .. } => Ty::Str,
            HirExprKind::CString { .. } => Ty::Pointer {
                volatile: false,
                inner: Box::new(Ty::Byte),
            },
            HirExprKind::Bool { .. } => Ty::Bool,
            HirExprKind::None => Ty::NoneLiteral,
            HirExprKind::Keyword { .. } | HirExprKind::ReaderForm { .. } => Ty::Unknown,
            HirExprKind::Name { reference } => self.type_of_name(reference.root),
            HirExprKind::Qualified { namespace, .. } => self.env.ty_from_ref(namespace),
            HirExprKind::Array { items } => {
                let mut element = Ty::Unknown;
                for item in items {
                    let t = self.check_expr(
                        item,
                        if element == Ty::Unknown {
                            None
                        } else {
                            Some(&element)
                        },
                    );
                    if element == Ty::Unknown {
                        element = self.materialize_literal(item.span, t);
                    } else {
                        self.require_assignable(item.span, &element, &t, "type/mismatch");
                    }
                }
                Ty::Array {
                    element: Box::new(element),
                }
            }
            HirExprKind::StructInit { namespace, .. } => self.env.ty_from_ref(namespace),
            HirExprKind::Unary { op, value } => {
                let v = self.check_expr(value, expected);
                self.check_unary(expr.span, *op, v)
            }
            HirExprKind::Binary { op, left, right } => {
                self.check_binary(expr.span, *op, left, right, expected)
            }
            HirExprKind::Call { callee, args } => self.check_call(expr.span, callee, args),
            HirExprKind::TypeCall { target, args } => self.check_type_call(expr.span, target, args),
            HirExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, None);
                if self.is_type_expr(index) {
                    self.diagnostic(
                        index.span,
                        "type/index-on-type",
                        "a type cannot be used as an index; Forge v1 does not support generic application syntax",
                    );
                } else {
                    self.check_expr(index, None);
                }
                match base_ty {
                    Ty::Array { element } | Ty::Slice { element, .. } => *element,
                    _ => Ty::Unknown,
                }
            }
            HirExprKind::Member { base, .. } => {
                self.check_expr(base, None);
                Ty::Unknown
            }
            HirExprKind::Try { value } => match self.check_expr(value, None) {
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
            HirExprKind::Closure {
                params,
                return_type,
                body,
                ..
            } => {
                let saved = std::mem::take(&mut self.local_types);
                let ptys: Vec<Ty> = params
                    .iter()
                    .map(|(id, t)| {
                        let ty = self.env.lower_hir_type(t);
                        self.local_types.insert(*id, ty.clone());
                        ty
                    })
                    .collect();
                let result = return_type
                    .as_ref()
                    .map(|t| self.env.lower_hir_type(t))
                    .unwrap_or(Ty::Unknown);
                let old_return = std::mem::replace(&mut self.expected_return, result.clone());
                self.check_block(body);
                self.expected_return = old_return;
                self.local_types.extend(saved);
                Ty::Closure {
                    params: ptys,
                    result: Box::new(result),
                }
            }
            HirExprKind::Match { value, arms } => {
                let matched = self.check_expr(value, None);
                let mut result = Ty::Unknown;
                for arm in arms {
                    self.bind_pattern_type(&arm.pattern, &matched);
                    if let Some(guard) = &arm.guard {
                        let g = self.check_expr(guard, Some(&Ty::Bool));
                        self.require_assignable(guard.span, &Ty::Bool, &g, "type/mismatch");
                    }
                    let arm_ty = match &arm.body {
                        crate::body_hir::HirMatchBody::Expr(e) => self.check_expr(
                            e,
                            if result == Ty::Unknown {
                                None
                            } else {
                                Some(&result)
                            },
                        ),
                        crate::body_hir::HirMatchBody::Block(b) => {
                            self.check_block(b);
                            Ty::Void
                        }
                    };
                    if result == Ty::Unknown {
                        result = self.materialize_literal(expr.span, arm_ty);
                    } else {
                        self.require_assignable(expr.span, &result, &arm_ty, "type/mismatch");
                    }
                }
                result
            }
            HirExprKind::Annotated { value, .. } => self.check_expr(value, expected),
            HirExprKind::Error => Ty::Error,
        };

        if let Some(expected) = expected {
            if matches!(ty, Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral)
                && self.is_assignable(expected, &ty)
            {
                ty = expected.clone();
            }
        }
        self.expressions.push(TypedExpr {
            span: expr.span,
            ty: ty.clone(),
            kind: TypedExprKind::Source { hir: expr.clone() },
        });
        ty
    }

    fn is_type_expr(&self, expr: &HirExpr) -> bool {
        match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::BuiltinType => true,
                ResolvedName::Def(id) => self.env.types.contains_key(&id),
                _ => false,
            },
            _ => false,
        }
    }

    fn check_assignment_target(&mut self, target: &HirExpr) {
        match &target.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Local(id) => {
                    if !self.mutable_locals.contains(&id) {
                        self.diagnostic(
                            target.span,
                            "assignment/immutable",
                            "cannot assign to a `val` binding",
                        );
                    }
                }
                _ => self.diagnostic(
                    target.span,
                    "assignment/invalid-target",
                    "assignment target is not a mutable local or writable place",
                ),
            },
            HirExprKind::Member { base, .. } | HirExprKind::Index { base, .. } => {
                self.check_assignment_target(base);
            }
            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => {
                let value_ty = self.check_expr(value, None);
                if matches!(value_ty, Ty::Reference { mutable: false, .. }) {
                    self.diagnostic(
                        target.span,
                        "assignment/immutable",
                        "cannot assign through an immutable reference",
                    );
                } else if !matches!(
                    value_ty,
                    Ty::Reference { mutable: true, .. } | Ty::Pointer { .. }
                ) {
                    self.diagnostic(
                        target.span,
                        "assignment/invalid-target",
                        "dereference assignment requires a pointer or reference",
                    );
                }
            }
            _ => self.diagnostic(
                target.span,
                "assignment/invalid-target",
                "expression is not a valid assignment target",
            ),
        }
    }

    fn type_of_name(&self, name: ResolvedName) -> Ty {
        match name {
            ResolvedName::Local(id) => self.local_types.get(&id).cloned().unwrap_or(Ty::Unknown),
            ResolvedName::Def(id) => self
                .env
                .functions
                .get(&id)
                .map(|sig| Ty::Function {
                    params: sig.params.iter().map(|p| p.ty.clone()).collect(),
                    result: Box::new(sig.result.clone()),
                    named_arguments: sig.named_arguments,
                })
                .unwrap_or(Ty::Unknown),
            ResolvedName::Error => Ty::Error,
            ResolvedName::Import(_) | ResolvedName::BuiltinType | ResolvedName::BuiltinValue => {
                Ty::Unknown
            }
        }
    }

    fn check_call(&mut self, span: Span, callee: &HirExpr, args: &[HirCallArg]) -> Ty {
        if let HirExprKind::Name { reference } = &callee.kind {
            if let ResolvedName::Def(id) = reference.root {
                if let Some(sig) = self.env.functions.get(&id).cloned() {
                    self.check_function_args(span, &sig, args);
                    return sig.result;
                }
            }
        }
        let callee_ty = self.check_expr(callee, None);
        match callee_ty {
            Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
                for (arg, param) in args.iter().zip(params.iter()) {
                    let value = arg_value(arg);
                    let actual = self.check_expr(value, Some(param));
                    self.require_assignable(value.span, param, &actual, "type/mismatch");
                }
                *result
            }
            Ty::Error => Ty::Error,
            _ => Ty::Unknown,
        }
    }

    fn check_function_args(&mut self, span: Span, sig: &FunctionSig, args: &[HirCallArg]) {
        let named = args.iter().any(|a| matches!(a, HirCallArg::Named { .. }));
        if named {
            if !sig.named_arguments {
                self.diagnostic(
                    span,
                    "call/unknown-name",
                    "named arguments require an nfn declaration",
                );
                return;
            }
            let mut seen = BTreeSet::new();
            for arg in args {
                let HirCallArg::Named { name, value } = arg else {
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
                if let Some(param) = sig.params.iter().find(|p| p.name == *name) {
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
            for param in &sig.params {
                if !param.has_default && !seen.contains(&param.name) {
                    self.diagnostic(
                        span,
                        "call/missing-argument",
                        format!("missing required argument `{}`", param.name),
                    );
                }
            }
        } else {
            for (index, arg) in args.iter().enumerate() {
                let value = arg_value(arg);
                if let Some(param) = sig.params.get(index) {
                    let actual = self.check_expr(value, Some(&param.ty));
                    self.require_assignable(value.span, &param.ty, &actual, "type/mismatch");
                } else {
                    self.diagnostic(value.span, "call/arity", "too many arguments");
                }
            }
            let required = sig.params.iter().filter(|p| !p.has_default).count();
            if args.len() < required {
                self.diagnostic(
                    span,
                    "call/arity",
                    format!(
                        "expected at least {required} arguments, found {}",
                        args.len()
                    ),
                );
            }
        }
    }

    fn check_type_call(&mut self, span: Span, target: &HirTypeRef, args: &[HirCallArg]) -> Ty {
        let target_ty = self.env.ty_from_ref(target);
        if args.len() != 1 {
            self.diagnostic(
                span,
                "call/arity",
                "type conversion/construction requires exactly one argument",
            );
            return Ty::Error;
        }
        let source = self.check_expr(arg_value(&args[0]), None);
        match &target_ty {
            Ty::Nominal(id) => {
                if let Some(underlying) = self.env.distinct_underlying(*id) {
                    if !self.is_explicitly_convertible(underlying, &source) {
                        self.diagnostic(
                            span,
                            "type/distinct",
                            format!("cannot construct distinct type from {source:?}"),
                        );
                    }
                }
            }
            Ty::Int { .. } | Ty::Float { .. } | Ty::Byte | Ty::Char => {
                if !self.is_explicitly_convertible(&target_ty, &source) {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("invalid explicit conversion from {source:?} to {target_ty:?}"),
                    );
                }
            }
            _ => {}
        }
        target_ty
    }

    fn check_binary(
        &mut self,
        span: Span,
        op: BinaryOp,
        left: &HirExpr,
        right: &HirExpr,
        expected: Option<&Ty>,
    ) -> Ty {
        let l = self.check_expr(left, expected);
        let r = self.check_expr(
            right,
            if is_concrete_numeric(&l) {
                Some(&l)
            } else {
                expected
            },
        );
        match op {
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr | BinaryOp::LogicalXor => {
                self.require_assignable(left.span, &Ty::Bool, &l, "type/mismatch");
                self.require_assignable(right.span, &Ty::Bool, &r, "type/mismatch");
                Ty::Bool
            }
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Less
            | BinaryOp::LessEq
            | BinaryOp::Greater
            | BinaryOp::GreaterEq => {
                if !self.compatible_binary(&l, &r) {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("incompatible operands: {l:?} and {r:?}"),
                    );
                }
                Ty::Bool
            }
            BinaryOp::BitAnd
            | BinaryOp::BitXor
            | BinaryOp::BitOr
            | BinaryOp::ShiftLeft
            | BinaryOp::ShiftRight => {
                if !is_integer_like(&l) || !is_integer_like(&r) || !self.compatible_binary(&l, &r) {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("bitwise operands must be compatible integers: {l:?}, {r:?}"),
                    );
                }
                self.common_numeric(l, r, expected)
            }
            BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => {
                if !is_numeric_like(&l) || !is_numeric_like(&r) || !self.compatible_binary(&l, &r) {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("arithmetic operands must have one numeric type: {l:?}, {r:?}"),
                    );
                }
                self.common_numeric(l, r, expected)
            }
        }
    }

    fn check_unary(&mut self, span: Span, op: UnaryOp, value: Ty) -> Ty {
        match op {
            UnaryOp::Neg if is_numeric_like(&value) => value,
            UnaryOp::Not if self.is_assignable(&Ty::Bool, &value) => Ty::Bool,
            UnaryOp::BitNot if is_integer_like(&value) => value,
            UnaryOp::AddressOf => Ty::Reference {
                mutable: false,
                inner: Box::new(value),
            },
            UnaryOp::AddressOfMut => Ty::Reference {
                mutable: true,
                inner: Box::new(value),
            },
            UnaryOp::Deref => match value {
                Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } => *inner,
                other => {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("cannot dereference {other:?}"),
                    );
                    Ty::Error
                }
            },
            _ => {
                self.diagnostic(
                    span,
                    "type/mismatch",
                    format!("invalid unary operand {value:?}"),
                );
                Ty::Error
            }
        }
    }

    fn mark_pattern_mutable(&mut self, pattern: &HirPattern) {
        match &pattern.kind {
            HirPatternKind::Binding { local, .. } => {
                self.mutable_locals.insert(*local);
            }
            HirPatternKind::As { local, pattern } => {
                self.mutable_locals.insert(*local);
                self.mark_pattern_mutable(pattern);
            }
            HirPatternKind::Some { value } => self.mark_pattern_mutable(value),
            HirPatternKind::Sequence { items, rest } => {
                for item in items {
                    self.mark_pattern_mutable(item);
                }
                if let Some(id) = rest {
                    self.mutable_locals.insert(*id);
                }
            }
            HirPatternKind::Or { patterns } => {
                for pattern in patterns {
                    self.mark_pattern_mutable(pattern);
                }
            }
            HirPatternKind::Variant { fields, .. } | HirPatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let Some(pattern) = &field.pattern {
                        self.mark_pattern_mutable(pattern);
                    }
                    if let Some(id) = field.shorthand_local {
                        self.mutable_locals.insert(id);
                    }
                }
            }
            HirPatternKind::Map { entries, .. } => {
                for entry in entries {
                    self.mutable_locals.insert(entry.local);
                }
            }
            HirPatternKind::Wildcard
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. } => {}
        }
    }

    fn bind_pattern_type(&mut self, pattern: &HirPattern, ty: &Ty) {
        match &pattern.kind {
            HirPatternKind::Binding { local, .. } => {
                self.local_types.insert(*local, ty.clone());
            }
            HirPatternKind::As { local, pattern } => {
                self.local_types.insert(*local, ty.clone());
                self.bind_pattern_type(pattern, ty);
            }
            HirPatternKind::Some { value } => {
                if let Ty::Optional { inner } = ty {
                    self.bind_pattern_type(value, inner);
                } else {
                    self.bind_pattern_type(value, &Ty::Unknown);
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let element = match ty {
                    Ty::Array { element } | Ty::Slice { element, .. } => element.as_ref().clone(),
                    _ => Ty::Unknown,
                };
                for item in items {
                    self.bind_pattern_type(item, &element);
                }
                if let Some(id) = rest {
                    self.local_types.insert(*id, ty.clone());
                }
            }
            HirPatternKind::Or { patterns } => {
                for p in patterns {
                    self.bind_pattern_type(p, ty);
                }
            }
            HirPatternKind::Variant { fields, .. } | HirPatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let Some(p) = &field.pattern {
                        self.bind_pattern_type(p, &Ty::Unknown);
                    }
                    if let Some(id) = field.shorthand_local {
                        self.local_types.insert(id, Ty::Unknown);
                    }
                }
            }
            HirPatternKind::Map { entries, .. } => {
                for e in entries {
                    self.local_types.insert(e.local, Ty::Unknown);
                }
            }
            HirPatternKind::Wildcard
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. } => {}
        }
    }

    fn require_assignable(&mut self, span: Span, expected: &Ty, actual: &Ty, code: &str) {
        if !self.is_assignable(expected, actual) {
            let code = if code == "type/mismatch"
                && (self.env.is_distinct_type(expected) || self.env.is_distinct_type(actual))
            {
                "type/distinct"
            } else {
                code
            };
            self.diagnostic(
                span,
                code,
                format!("expected {expected:?}, found {actual:?}"),
            );
        }
    }

    fn is_assignable(&self, expected: &Ty, actual: &Ty) -> bool {
        if matches!(expected, Ty::Unknown | Ty::Error) || matches!(actual, Ty::Unknown | Ty::Error)
        {
            return true;
        }
        if expected == actual {
            return true;
        }
        match (expected, actual) {
            (Ty::Int { .. }, Ty::IntLiteral) | (Ty::Float { .. }, Ty::FloatLiteral) => true,
            (Ty::Optional { inner }, Ty::NoneLiteral) => {
                let _ = inner;
                true
            }
            (Ty::Optional { inner }, other) => self.is_assignable(inner, other),
            _ => false,
        }
    }

    fn compatible_binary(&self, a: &Ty, b: &Ty) -> bool {
        if a == b {
            return true;
        }
        matches!(
            (a, b),
            (Ty::Int { .. }, Ty::IntLiteral)
                | (Ty::IntLiteral, Ty::Int { .. })
                | (Ty::Float { .. }, Ty::FloatLiteral)
                | (Ty::FloatLiteral, Ty::Float { .. })
                | (Ty::IntLiteral, Ty::IntLiteral)
                | (Ty::FloatLiteral, Ty::FloatLiteral)
        )
    }

    fn common_numeric(&mut self, a: Ty, b: Ty, expected: Option<&Ty>) -> Ty {
        if is_concrete_numeric(&a) {
            a
        } else if is_concrete_numeric(&b) {
            b
        } else if let Some(e) = expected {
            e.clone()
        } else {
            self.materialize_literal(Span::new(0, 0), a)
        }
    }

    fn materialize_literal(&mut self, span: Span, ty: Ty) -> Ty {
        match ty {
            Ty::IntLiteral => {
                self.diagnostic(
                    span,
                    "type/ambiguous-literal",
                    "integer literal requires a concrete type context or suffix",
                );
                Ty::Error
            }
            Ty::FloatLiteral => {
                self.diagnostic(
                    span,
                    "type/ambiguous-literal",
                    "float literal requires a concrete type context or suffix",
                );
                Ty::Error
            }
            other => other,
        }
    }

    fn is_explicitly_convertible(&self, target: &Ty, source: &Ty) -> bool {
        if self.is_assignable(target, source) {
            return true;
        }
        if is_numeric_concrete(target) && is_numeric_concrete(source) {
            return true;
        }
        if let Ty::Nominal(id) = source {
            if let Some(inner) = self.env.distinct_underlying(*id) {
                return self.is_explicitly_convertible(target, inner);
            }
        }
        false
    }
}

fn arg_value(arg: &HirCallArg) -> &HirExpr {
    match arg {
        HirCallArg::Positional { value } | HirCallArg::Named { value, .. } => value,
    }
}

fn builtin_ty(name: &str) -> Option<Ty> {
    Some(match name {
        "bool" => Ty::Bool,
        "char" => Ty::Char,
        "str" => Ty::Str,
        "byte" => Ty::Byte,
        "void" => Ty::Void,
        "never" => Ty::Never,
        "i8" => Ty::Int {
            signed: true,
            width: IntWidth::W8,
        },
        "i16" => Ty::Int {
            signed: true,
            width: IntWidth::W16,
        },
        "i32" => Ty::Int {
            signed: true,
            width: IntWidth::W32,
        },
        "i64" => Ty::Int {
            signed: true,
            width: IntWidth::W64,
        },
        "isize" => Ty::Int {
            signed: true,
            width: IntWidth::Pointer,
        },
        "u8" => Ty::Int {
            signed: false,
            width: IntWidth::W8,
        },
        "u16" => Ty::Int {
            signed: false,
            width: IntWidth::W16,
        },
        "u32" => Ty::Int {
            signed: false,
            width: IntWidth::W32,
        },
        "u64" => Ty::Int {
            signed: false,
            width: IntWidth::W64,
        },
        "usize" => Ty::Int {
            signed: false,
            width: IntWidth::Pointer,
        },
        "f32" => Ty::Float { bits: 32 },
        "f64" => Ty::Float { bits: 64 },
        _ => return None,
    })
}

fn integer_literal_ty(text: &str) -> Ty {
    let suffix = [
        ("isize", true, IntWidth::Pointer),
        ("usize", false, IntWidth::Pointer),
        ("i64", true, IntWidth::W64),
        ("u64", false, IntWidth::W64),
        ("i32", true, IntWidth::W32),
        ("u32", false, IntWidth::W32),
        ("i16", true, IntWidth::W16),
        ("u16", false, IntWidth::W16),
        ("i8", true, IntWidth::W8),
        ("u8", false, IntWidth::W8),
    ];
    for (s, signed, width) in suffix {
        if text.ends_with(s) {
            return Ty::Int { signed, width };
        }
    }
    Ty::IntLiteral
}
fn float_literal_ty(text: &str) -> Ty {
    if text.ends_with("f32") {
        Ty::Float { bits: 32 }
    } else if text.ends_with("f64") {
        Ty::Float { bits: 64 }
    } else {
        Ty::FloatLiteral
    }
}
fn is_integer_like(t: &Ty) -> bool {
    matches!(t, Ty::Int { .. } | Ty::IntLiteral | Ty::Byte)
}
fn is_numeric_like(t: &Ty) -> bool {
    is_integer_like(t) || matches!(t, Ty::Float { .. } | Ty::FloatLiteral)
}
fn is_numeric_concrete(t: &Ty) -> bool {
    matches!(t, Ty::Int { .. } | Ty::Float { .. } | Ty::Byte)
}
fn is_concrete_numeric(t: &Ty) -> bool {
    is_numeric_concrete(t)
}
