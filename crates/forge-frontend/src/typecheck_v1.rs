use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{self, BinaryOp, DeclKind, Span, UnaryOp},
    body_hir::{
        BodyHirOutput, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern, HirPatternKind,
        HirStmt, HirStmtKind, HirType, HirTypeKind, HirTypeRef,
    },
    hir::{DefId, HirModule, MetadataTable},
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
    pub metadata: MetadataTable,
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
struct FieldInfo {
    ty: Ty,
    has_default: bool,
}

#[derive(Debug, Clone)]
enum TypeInfoKind {
    Distinct(Ty),
    Alias(Ty),
    Struct(BTreeMap<String, FieldInfo>),
    Enum(BTreeSet<String>),
    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),
    Nominal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberLookup {
    Field(Ty),
    MissingField,
    Unsupported,
    Unknown,
}

pub fn type_check_module(
    source: &ast::SourceFile,
    module: &HirModule,
    bodies: &BodyHirOutput,
) -> TypeCheckOutput {
    let mut output = TypeCheckOutput {
        metadata: module.metadata.clone(),
        ..TypeCheckOutput::default()
    };
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

        // Once aliases/distinct types are known, record structural metadata used by
        // member access, construction and pattern typing.
        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            match &declaration.kind.kind {
                DeclKind::Struct(value) => {
                    let fields = value
                        .fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                FieldInfo {
                                    ty: env.lower_ast_type(&field.ty, module),
                                    has_default: field.default.is_some(),
                                },
                            )
                        })
                        .collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Struct(fields),
                        },
                    );
                }
                DeclKind::Enum(value) => {
                    let variants = value
                        .variants
                        .iter()
                        .map(|variant| variant.name.clone())
                        .collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Enum(variants),
                        },
                    );
                }
                DeclKind::Tagged(value) => {
                    let variants = value
                        .variants
                        .iter()
                        .map(|variant| {
                            let fields = variant
                                .fields
                                .iter()
                                .map(|field| {
                                    (
                                        field.name.clone(),
                                        FieldInfo {
                                            ty: env.lower_ast_type(&field.ty, module),
                                            has_default: field.default.is_some(),
                                        },
                                    )
                                })
                                .collect();
                            (variant.name.clone(), fields)
                        })
                        .collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Tagged(variants),
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
            Some(TypeInfoKind::Distinct(_))
            | Some(TypeInfoKind::Struct(_))
            | Some(TypeInfoKind::Enum(_))
            | Some(TypeInfoKind::Tagged(_))
            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),
            None => Ty::Unknown,
        }
    }

    fn lookup_member(&self, ty: &Ty, name: &str) -> MemberLookup {
        match ty {
            Ty::Reference { inner, .. } => self.lookup_member(inner, name),
            Ty::Nominal(id) => match self.types.get(id).map(|info| &info.kind) {
                Some(TypeInfoKind::Struct(fields)) => fields
                    .get(name)
                    .map(|field| MemberLookup::Field(field.ty.clone()))
                    .unwrap_or(MemberLookup::MissingField),
                Some(_) => MemberLookup::Unsupported,
                None => MemberLookup::Unknown,
            },
            Ty::Array { .. } | Ty::Slice { .. } | Ty::Str if name == "len" => {
                MemberLookup::Field(Ty::Int {
                    signed: false,
                    width: IntWidth::Pointer,
                })
            }
            Ty::Unknown | Ty::Error => MemberLookup::Unknown,
            _ => MemberLookup::Unsupported,
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
                self.check_irrefutable_binding_pattern(pattern, &final_ty);
                self.check_pattern(pattern, &final_ty);
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
                self.check_pattern(pattern, &element);
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
            HirExprKind::Qualified { namespace, name } => {
                let ty = self.env.ty_from_ref(namespace);
                self.check_qualified_variant(expr.span, &ty, name);
                ty
            }
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
            HirExprKind::StructInit {
                namespace,
                variant,
                fields,
            } => {
                let ty = self.env.ty_from_ref(namespace);
                self.check_struct_init(expr.span, &ty, variant.as_deref(), fields);
                ty
            }
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
            HirExprKind::Member { base, name } => {
                let base_ty = self.check_expr(base, None);
                self.check_member(expr.span, &base_ty, name)
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
                    self.check_pattern(&arm.pattern, &matched);
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
            HirExprKind::Member { base, .. } => match self.place_type(base) {
                Ty::Reference { mutable: true, .. } => {}
                Ty::Reference { mutable: false, .. } => self.diagnostic(
                    target.span,
                    "assignment/immutable",
                    "cannot assign through an immutable reference",
                ),
                _ => self.check_assignment_target(base),
            },
            HirExprKind::Index { base, .. } => match self.place_type(base) {
                Ty::Slice { mutable: true, .. } => {}
                Ty::Slice { mutable: false, .. } => self.diagnostic(
                    target.span,
                    "assignment/immutable",
                    "cannot assign through a read-only slice",
                ),
                Ty::Reference { mutable: true, .. } => {}
                Ty::Reference { mutable: false, .. } => self.diagnostic(
                    target.span,
                    "assignment/immutable",
                    "cannot assign through an immutable reference",
                ),
                _ => self.check_assignment_target(base),
            },
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

    fn place_type(&self, expr: &HirExpr) -> Ty {
        match &expr.kind {
            HirExprKind::Name { reference } => self.type_of_name(reference.root),
            HirExprKind::Member { base, name } => {
                match self.env.lookup_member(&self.place_type(base), name) {
                    MemberLookup::Field(ty) => ty,
                    _ => Ty::Unknown,
                }
            }
            HirExprKind::Index { base, .. } => match self.place_type(base) {
                Ty::Array { element } | Ty::Slice { element, .. } => *element,
                Ty::Reference { inner, .. } => match *inner {
                    Ty::Array { element } | Ty::Slice { element, .. } => *element,
                    _ => Ty::Unknown,
                },
                _ => Ty::Unknown,
            },
            HirExprKind::Unary { op, value } => match op {
                UnaryOp::AddressOf => Ty::Reference {
                    mutable: false,
                    inner: Box::new(self.place_type(value)),
                },
                UnaryOp::AddressOfMut => Ty::Reference {
                    mutable: true,
                    inner: Box::new(self.place_type(value)),
                },
                UnaryOp::Deref => match self.place_type(value) {
                    Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } => *inner,
                    _ => Ty::Unknown,
                },
                _ => Ty::Unknown,
            },
            _ => Ty::Unknown,
        }
    }

    fn check_member(&mut self, span: Span, base_ty: &Ty, name: &str) -> Ty {
        match self.env.lookup_member(base_ty, name) {
            MemberLookup::Field(ty) => ty,
            MemberLookup::MissingField => {
                self.diagnostic(
                    span,
                    "type/unknown-field",
                    format!("type {base_ty:?} has no field `{name}`"),
                );
                Ty::Error
            }
            MemberLookup::Unsupported => {
                self.diagnostic(
                    span,
                    "type/member",
                    format!("member access `{name}` is not valid on {base_ty:?}"),
                );
                Ty::Error
            }
            MemberLookup::Unknown => Ty::Unknown,
        }
    }

    fn check_qualified_variant(&mut self, span: Span, ty: &Ty, name: &str) {
        let Ty::Nominal(id) = ty else {
            return;
        };
        let kind = self.env.types.get(id).map(|info| info.kind.clone());
        match kind {
            Some(TypeInfoKind::Enum(variants)) => {
                if !variants.contains(name) {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown enum variant `{name}`"),
                    );
                }
            }
            Some(TypeInfoKind::Tagged(variants)) => {
                if !variants.contains_key(name) {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown tagged-union variant `{name}`"),
                    );
                }
            }
            _ => self.diagnostic(
                span,
                "type/unknown-variant",
                format!("{ty:?} does not define variants"),
            ),
        }
    }

    fn check_struct_init(
        &mut self,
        span: Span,
        ty: &Ty,
        variant: Option<&str>,
        fields: &[(String, HirExpr)],
    ) {
        let Ty::Nominal(id) = ty else {
            for (_, value) in fields {
                self.check_expr(value, None);
            }
            return;
        };
        let kind = self.env.types.get(id).map(|info| info.kind.clone());
        match kind {
            Some(TypeInfoKind::Struct(defs)) => {
                if let Some(name) = variant {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("struct type does not define variant `{name}`"),
                    );
                }
                self.check_init_fields(span, fields, &defs);
            }
            Some(TypeInfoKind::Tagged(variants)) => {
                let Some(name) = variant else {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        "tagged-union construction requires a qualified variant",
                    );
                    for (_, value) in fields {
                        self.check_expr(value, None);
                    }
                    return;
                };
                if let Some(defs) = variants.get(name) {
                    self.check_init_fields(span, fields, defs);
                } else {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown tagged-union variant `{name}`"),
                    );
                    for (_, value) in fields {
                        self.check_expr(value, None);
                    }
                }
            }
            _ => {
                self.diagnostic(
                    span,
                    "type/constructor",
                    format!("brace construction is not valid for {ty:?}"),
                );
                for (_, value) in fields {
                    self.check_expr(value, None);
                }
            }
        }
    }

    fn check_init_fields(
        &mut self,
        span: Span,
        fields: &[(String, HirExpr)],
        defs: &BTreeMap<String, FieldInfo>,
    ) {
        let mut seen = BTreeSet::new();
        for (name, value) in fields {
            if !seen.insert(name.clone()) {
                self.diagnostic(
                    value.span,
                    "type/duplicate-field",
                    format!("duplicate field `{name}`"),
                );
                continue;
            }
            if let Some(field) = defs.get(name) {
                let actual = self.check_expr(value, Some(&field.ty));
                self.require_assignable(value.span, &field.ty, &actual, "type/mismatch");
            } else {
                self.diagnostic(
                    value.span,
                    "type/unknown-field",
                    format!("unknown field `{name}`"),
                );
                self.check_expr(value, None);
            }
        }
        for (name, field) in defs {
            if !field.has_default && !seen.contains(name) {
                self.diagnostic(
                    span,
                    "type/missing-field",
                    format!("missing required field `{name}`"),
                );
            }
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

    fn check_irrefutable_binding_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
        if !self.pattern_is_irrefutable(pattern, ty) {
            self.diagnostic(
                pattern.span,
                "pattern/refutable-binding",
                "destructuring declarations require a statically irrefutable pattern",
            );
        }
    }

    fn pattern_is_irrefutable(&self, pattern: &HirPattern, ty: &Ty) -> bool {
        match &pattern.kind {
            HirPatternKind::Wildcard | HirPatternKind::Binding { .. } => true,
            HirPatternKind::As { pattern, .. } => self.pattern_is_irrefutable(pattern, ty),
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                if !matches!(expected, Ty::Unknown | Ty::Error)
                    && !matches!(ty, Ty::Unknown | Ty::Error)
                    && expected != *ty
                {
                    // The ordinary pattern type diagnostic owns this mismatch.
                    return true;
                }
                let Ty::Nominal(id) = expected else {
                    return true;
                };
                let Some(TypeInfoKind::Struct(defs)) =
                    self.env.types.get(&id).map(|info| &info.kind)
                else {
                    return true;
                };
                fields.iter().all(|field| {
                    let Some(info) = defs.get(&field.name) else {
                        return true;
                    };
                    field
                        .pattern
                        .as_ref()
                        .is_none_or(|nested| self.pattern_is_irrefutable(nested, &info.ty))
                })
            }
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                if !matches!(expected, Ty::Unknown | Ty::Error)
                    && !matches!(ty, Ty::Unknown | Ty::Error)
                    && expected != *ty
                {
                    return true;
                }
                let Ty::Nominal(id) = expected else {
                    return false;
                };
                match self.env.types.get(&id).map(|info| &info.kind) {
                    Some(TypeInfoKind::Enum(variants)) => {
                        variants.len() == 1 && variants.contains(name) && fields.is_empty()
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        if variants.len() != 1 {
                            return false;
                        }
                        let Some(defs) = variants.get(name) else {
                            return false;
                        };
                        fields.iter().all(|field| {
                            let Some(info) = defs.get(&field.name) else {
                                return true;
                            };
                            field
                                .pattern
                                .as_ref()
                                .is_none_or(|nested| self.pattern_is_irrefutable(nested, &info.ty))
                        })
                    }
                    _ => false,
                }
            }
            HirPatternKind::Or { patterns } => patterns
                .iter()
                .any(|branch| self.pattern_is_irrefutable(branch, ty)),
            // Length-sensitive sequence declarations require fixed-array length to be
            // retained in Ty. Until then, sequence declarations cannot be proven safe.
            HirPatternKind::Sequence { .. }
            | HirPatternKind::Map { .. }
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. }
            | HirPatternKind::Some { .. } => false,
        }
    }

    fn check_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
        let mut bindings = BTreeMap::new();
        self.collect_pattern_bindings(pattern, ty, &mut bindings);
        for (local, binding_ty) in bindings {
            self.local_types.insert(local, binding_ty);
        }
    }

    fn collect_pattern_bindings(
        &mut self,
        pattern: &HirPattern,
        ty: &Ty,
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        match &pattern.kind {
            HirPatternKind::Binding { local, .. } => {
                out.insert(*local, ty.clone());
            }
            HirPatternKind::As { local, pattern } => {
                out.insert(*local, ty.clone());
                self.collect_pattern_bindings(pattern, ty, out);
            }
            HirPatternKind::Literal { value } => {
                self.check_pattern_literal(pattern.span, value, ty);
            }
            HirPatternKind::Range { start, end, .. } => {
                self.check_pattern_literal(pattern.span, start, ty);
                self.check_pattern_literal(pattern.span, end, ty);
                if !matches!(ty, Ty::Unknown | Ty::Error | Ty::Int { .. } | Ty::Char) {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!(
                            "range pattern requires an integer or char scrutinee, found {ty:?}"
                        ),
                    );
                }
            }
            HirPatternKind::Some { value } => match ty {
                Ty::Optional { inner } => self.collect_pattern_bindings(value, inner, out),
                Ty::Unknown | Ty::Error => self.collect_pattern_bindings(value, &Ty::Unknown, out),
                _ => {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!("Some(...) pattern requires an optional scrutinee, found {ty:?}"),
                    );
                    self.collect_pattern_bindings(value, &Ty::Unknown, out);
                }
            },
            HirPatternKind::None { .. } => {
                if !matches!(ty, Ty::Optional { .. } | Ty::Unknown | Ty::Error) {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!("None pattern requires an optional scrutinee, found {ty:?}"),
                    );
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let element = match ty {
                    Ty::Array { element } | Ty::Slice { element, .. } => element.as_ref().clone(),
                    Ty::Unknown | Ty::Error => Ty::Unknown,
                    _ => {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("sequence pattern requires an array or slice, found {ty:?}"),
                        );
                        Ty::Unknown
                    }
                };
                for item in items {
                    self.collect_pattern_bindings(item, &element, out);
                }
                if let Some(id) = rest {
                    out.insert(*id, ty.clone());
                }
            }
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                self.require_pattern_type(pattern.span, &expected, ty);
                let defs = match &expected {
                    Ty::Nominal(id) => match self.env.types.get(id).map(|info| info.kind.clone()) {
                        Some(TypeInfoKind::Struct(fields)) => Some(fields),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(defs) = defs {
                    self.collect_record_pattern_fields(pattern.span, fields, &defs, out);
                } else {
                    if !matches!(expected, Ty::Unknown | Ty::Error) {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("struct pattern requires a struct type, found {expected:?}"),
                        );
                    }
                    self.collect_unknown_pattern_fields(fields, out);
                }
            }
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                self.require_pattern_type(pattern.span, &expected, ty);
                let kind = match &expected {
                    Ty::Nominal(id) => self.env.types.get(id).map(|info| info.kind.clone()),
                    _ => None,
                };
                match kind {
                    Some(TypeInfoKind::Enum(variants)) => {
                        if !variants.contains(name) {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-variant",
                                format!("unknown enum variant `{name}`"),
                            );
                        }
                        if !fields.is_empty() {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-field",
                                format!("enum variant `{name}` has no payload fields"),
                            );
                            self.collect_unknown_pattern_fields(fields, out);
                        }
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        if let Some(defs) = variants.get(name) {
                            self.collect_record_pattern_fields(pattern.span, fields, defs, out);
                        } else {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-variant",
                                format!("unknown tagged-union variant `{name}`"),
                            );
                            self.collect_unknown_pattern_fields(fields, out);
                        }
                    }
                    Some(_) => {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("variant pattern requires an enum or tagged union, found {expected:?}"),
                        );
                        self.collect_unknown_pattern_fields(fields, out);
                    }
                    None => self.collect_unknown_pattern_fields(fields, out),
                }
            }
            HirPatternKind::Map { entries, .. } => {
                // Map-pattern typing depends on the collection pattern protocol, which is
                // intentionally not modeled in this primitive type environment yet.
                for entry in entries {
                    out.insert(entry.local, Ty::Unknown);
                }
            }
            HirPatternKind::Or { patterns } => {
                let mut merged: Option<BTreeMap<LocalId, Ty>> = None;
                for alternative in patterns {
                    let mut branch = BTreeMap::new();
                    self.collect_pattern_bindings(alternative, ty, &mut branch);
                    if let Some(current) = &mut merged {
                        let current_ids = current.keys().copied().collect::<BTreeSet<_>>();
                        let branch_ids = branch.keys().copied().collect::<BTreeSet<_>>();
                        if current_ids != branch_ids {
                            self.diagnostic(
                                alternative.span,
                                "pattern/or-bindings",
                                "OR-pattern alternatives must bind the same locals",
                            );
                        }
                        for (local, branch_ty) in branch {
                            match current.get(&local).cloned() {
                                Some(current_ty)
                                    if !matches!(current_ty, Ty::Unknown | Ty::Error)
                                        && !matches!(branch_ty, Ty::Unknown | Ty::Error)
                                        && current_ty != branch_ty =>
                                {
                                    self.diagnostic(
                                        alternative.span,
                                        "pattern/or-binding-type",
                                        format!(
                                            "OR-pattern binding {local:?} has incompatible types {current_ty:?} and {branch_ty:?}"
                                        ),
                                    );
                                }
                                Some(current_ty)
                                    if matches!(current_ty, Ty::Unknown | Ty::Error)
                                        && !matches!(branch_ty, Ty::Unknown | Ty::Error) =>
                                {
                                    current.insert(local, branch_ty);
                                }
                                None => {
                                    current.insert(local, branch_ty);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        merged = Some(branch);
                    }
                }
                if let Some(merged) = merged {
                    out.extend(merged);
                }
            }
            HirPatternKind::Wildcard => {}
        }
    }

    fn collect_record_pattern_fields(
        &mut self,
        span: Span,
        fields: &[crate::body_hir::HirPatternField],
        defs: &BTreeMap<String, FieldInfo>,
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        let mut seen = BTreeSet::new();
        for field in fields {
            if !seen.insert(field.name.clone()) {
                self.diagnostic(
                    span,
                    "pattern/duplicate-field",
                    format!("duplicate pattern field `{}`", field.name),
                );
                continue;
            }
            let Some(info) = defs.get(&field.name) else {
                self.diagnostic(
                    span,
                    "pattern/unknown-field",
                    format!("unknown pattern field `{}`", field.name),
                );
                if let Some(pattern) = &field.pattern {
                    self.collect_pattern_bindings(pattern, &Ty::Unknown, out);
                }
                if let Some(local) = field.shorthand_local {
                    out.insert(local, Ty::Unknown);
                }
                continue;
            };
            if let Some(pattern) = &field.pattern {
                self.collect_pattern_bindings(pattern, &info.ty, out);
            }
            if let Some(local) = field.shorthand_local {
                out.insert(local, info.ty.clone());
            }
        }
    }

    fn collect_unknown_pattern_fields(
        &mut self,
        fields: &[crate::body_hir::HirPatternField],
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        for field in fields {
            if let Some(pattern) = &field.pattern {
                self.collect_pattern_bindings(pattern, &Ty::Unknown, out);
            }
            if let Some(local) = field.shorthand_local {
                out.insert(local, Ty::Unknown);
            }
        }
    }

    fn check_pattern_literal(&mut self, span: Span, value: &ast::PatternLiteral, ty: &Ty) {
        if matches!(ty, Ty::Unknown | Ty::Error) {
            return;
        }
        let literal_ty = pattern_literal_ty(value);
        if !self.is_assignable(ty, &literal_ty) {
            self.diagnostic(
                span,
                "pattern/type",
                format!("pattern literal {literal_ty:?} is incompatible with {ty:?}"),
            );
        }
    }

    fn require_pattern_type(&mut self, span: Span, expected: &Ty, actual: &Ty) {
        if matches!(expected, Ty::Unknown | Ty::Error) || matches!(actual, Ty::Unknown | Ty::Error)
        {
            return;
        }
        if expected != actual {
            self.diagnostic(
                span,
                "pattern/type",
                format!("pattern expects {expected:?}, found scrutinee {actual:?}"),
            );
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

fn pattern_literal_ty(value: &ast::PatternLiteral) -> Ty {
    match value {
        ast::PatternLiteral::Integer { text } => integer_literal_ty(text),
        ast::PatternLiteral::Character { .. } => Ty::Char,
        ast::PatternLiteral::String { .. } => Ty::Str,
        ast::PatternLiteral::Bool { .. } => Ty::Bool,
    }
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
