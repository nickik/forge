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
        length: Option<u64>,
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
    Source {
        hir: HirExpr,
    },
    ResolvedCall {
        target: DefId,
        method: bool,
        hir: HirExpr,
    },
    OptionalPromote {
        value: Box<TypedExpr>,
    },
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
    validate_declaration_array_lengths(source, &mut output.diagnostics);
    let env = ModuleTypeEnv::build(source, module);

    for default in &bodies.field_defaults {
        let mut checker = BodyChecker::new(&env, Ty::Void, &mut output.diagnostics);
        let expected = env.lower_hir_type(&default.expected);
        let actual = checker.check_expr(&default.value, Some(&expected));
        checker.require_assignable(
            default.value.span,
            &expected,
            &actual,
            "type/declaration-default",
        );
    }

    for explicit in &bodies.enum_values {
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

    for (owner, body) in &bodies.functions {
        let expected_return = env
            .functions
            .get(owner)
            .map(|sig| sig.result.clone())
            .unwrap_or(Ty::Unknown);
        let mut checker = BodyChecker::new(&env, expected_return, &mut output.diagnostics);
        for (local, ty) in &body.params {
            let param_ty = env.lower_hir_type(ty);
            if array_type_has_unknown_length(&param_ty) {
                checker.diagnostic(
                    ty.span,
                    "type/array-length",
                    "array length must be a non-negative compile-time integer",
                );
            }
            if let Some(default) = body.param_defaults.get(local) {
                let actual = checker.check_expr(default, Some(&param_ty));
                checker.require_assignable(
                    default.span,
                    &param_ty,
                    &actual,
                    "type/declaration-default",
                );
            }
            checker.local_types.insert(*local, param_ty);
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
    methods: BTreeMap<(DefId, String), DefId>,
}

impl ModuleTypeEnv {
    fn build(source: &ast::SourceFile, module: &HirModule) -> Self {
        let mut env = Self {
            types: BTreeMap::new(),
            functions: BTreeMap::new(),
            methods: BTreeMap::new(),
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

        for (index, declaration) in source.declarations.iter().enumerate() {
            let impl_owner = DefId(index as u32);
            let DeclKind::Impl(value) = &declaration.kind.kind else {
                continue;
            };
            let Some(target_name) = value.target.segments.first() else {
                continue;
            };
            let Some(target_id) = module.symbols.get(target_name).and_then(|set| set.type_def)
            else {
                continue;
            };
            let method_defs = module
                .methods
                .iter()
                .filter(|method| method.impl_owner == impl_owner)
                .collect::<Vec<_>>();
            for (method, method_def) in value.methods.iter().zip(method_defs) {
                let params = method
                    .function
                    .params
                    .iter()
                    .map(|p| ParamSig {
                        name: p.name.clone(),
                        ty: env.lower_ast_type(&p.ty, module),
                        has_default: p.default.is_some(),
                    })
                    .collect::<Vec<_>>();
                let result = method
                    .function
                    .return_type
                    .as_ref()
                    .map(|t| env.lower_ast_type(t, module))
                    .unwrap_or(Ty::Void);
                env.functions.insert(
                    method_def.id,
                    FunctionSig {
                        params: params.clone(),
                        result,
                        named_arguments: method.function.named_arguments,
                    },
                );
                if params.first().is_some_and(|param| param.name == "self") {
                    env.methods
                        .insert((target_id, method.function.name.clone()), method_def.id);
                }
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
            ast::TypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
                length: eval_const_usize_ast(length),
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
            HirTypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
                length: eval_const_usize_hir(length),
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

    fn lookup_method(&self, ty: &Ty, name: &str) -> Option<(DefId, &FunctionSig)> {
        let id = match ty {
            Ty::Reference { inner, .. } => match inner.as_ref() {
                Ty::Nominal(id) => *id,
                _ => return None,
            },
            Ty::Nominal(id) => *id,
            _ => return None,
        };
        let method = *self.methods.get(&(id, name.to_owned()))?;
        self.functions.get(&method).map(|sig| (method, sig))
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
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => *element,
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
        let mut resolved_call: Option<(DefId, bool)> = None;
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
                    length: Some(items.len() as u64),
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
            HirExprKind::Call { callee, args } => {
                let (result, target) = self.check_call(expr.span, callee, args);
                resolved_call = target;
                result
            }
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
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => *element,
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
                                expected
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
                self.check_match_exhaustiveness(expr.span, &matched, arms);
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
        let kind = resolved_call
            .map(|(target, method)| TypedExprKind::ResolvedCall {
                target,
                method,
                hir: expr.clone(),
            })
            .unwrap_or_else(|| TypedExprKind::Source { hir: expr.clone() });
        self.expressions.push(TypedExpr {
            span: expr.span,
            ty: ty.clone(),
            kind,
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
                Ty::Array { element, .. } | Ty::Slice { element, .. } => *element,
                Ty::Reference { inner, .. } => match *inner {
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => *element,
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

    fn check_call(
        &mut self,
        span: Span,
        callee: &HirExpr,
        args: &[HirCallArg],
    ) -> (Ty, Option<(DefId, bool)>) {
        if let HirExprKind::Member { base, name } = &callee.kind {
            let receiver_ty = self.check_expr(base, None);
            if let Some((method_id, sig)) = self.env.lookup_method(&receiver_ty, name) {
                let sig = sig.clone();
                self.check_method_receiver(base, &receiver_ty, &sig);
                let reduced = FunctionSig {
                    params: sig.params.iter().skip(1).cloned().collect(),
                    result: sig.result.clone(),
                    named_arguments: sig.named_arguments,
                };
                self.check_function_args(span, &reduced, args);
                return (sig.result, Some((method_id, true)));
            }
        }
        if let HirExprKind::Name { reference } = &callee.kind {
            if let ResolvedName::Def(id) = reference.root {
                if let Some(sig) = self.env.functions.get(&id).cloned() {
                    self.check_function_args(span, &sig, args);
                    return (sig.result, Some((id, false)));
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
                (*result, None)
            }
            Ty::Error => (Ty::Error, None),
            _ => (Ty::Unknown, None),
        }
    }

    fn check_method_receiver(&mut self, receiver: &HirExpr, actual: &Ty, sig: &FunctionSig) {
        let Some(self_param) = sig.params.first() else {
            self.diagnostic(
                receiver.span,
                "method/receiver",
                "method call target has no `self` parameter",
            );
            return;
        };
        match &self_param.ty {
            Ty::Reference { mutable, inner } => {
                let compatible = match actual {
                    Ty::Reference {
                        mutable: actual_mutable,
                        inner: actual_inner,
                    } => actual_inner.as_ref() == inner.as_ref() && (!*mutable || *actual_mutable),
                    other => other == inner.as_ref(),
                };
                if !compatible {
                    self.diagnostic(
                        receiver.span,
                        "method/receiver",
                        format!(
                            "method receiver expects {:?}, found {:?}",
                            self_param.ty, actual
                        ),
                    );
                } else if *mutable
                    && !matches!(actual, Ty::Reference { mutable: true, .. })
                    && !self.is_mutable_place(receiver)
                {
                    self.diagnostic(
                        receiver.span,
                        "method/immutable-receiver",
                        "method requires a mutable receiver",
                    );
                }
            }
            expected => self.require_assignable(receiver.span, expected, actual, "method/receiver"),
        }
    }

    fn is_mutable_place(&self, expr: &HirExpr) -> bool {
        match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Local(id) => self.mutable_locals.contains(&id),
                _ => false,
            },
            HirExprKind::Member { base, .. } | HirExprKind::Index { base, .. } => {
                match self.place_type(base) {
                    Ty::Reference { mutable, .. } => mutable,
                    Ty::Slice { mutable, .. } => mutable,
                    _ => self.is_mutable_place(base),
                }
            }
            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => matches!(
                self.place_type(value),
                Ty::Reference { mutable: true, .. } | Ty::Pointer { .. }
            ),
            _ => false,
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
            HirPatternKind::Sequence { items, rest } => match ty {
                Ty::Array {
                    element,
                    length: Some(length),
                } => {
                    let enough = if rest.is_some() {
                        *length >= items.len() as u64
                    } else {
                        *length == items.len() as u64
                    };
                    enough
                        && items
                            .iter()
                            .all(|item| self.pattern_is_irrefutable(item, element))
                }
                _ => false,
            },
            HirPatternKind::Map { .. }
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. }
            | HirPatternKind::Some { .. } => false,
        }
    }

    fn check_match_exhaustiveness(
        &mut self,
        span: Span,
        ty: &Ty,
        arms: &[crate::body_hir::HirMatchArm],
    ) {
        let Some(required) = self.finite_match_cases(ty) else {
            return;
        };
        let mut covered = BTreeSet::new();
        for arm in arms {
            if arm.guard.is_some() {
                continue;
            }
            covered.extend(self.pattern_match_cases(&arm.pattern, ty, &required));
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

    fn finite_match_cases(&self, ty: &Ty) -> Option<BTreeSet<String>> {
        match ty {
            Ty::Bool => Some(
                ["false".to_owned(), "true".to_owned()]
                    .into_iter()
                    .collect(),
            ),
            Ty::Optional { .. } => {
                Some(["None".to_owned(), "Some".to_owned()].into_iter().collect())
            }
            Ty::Nominal(id) => match self.env.types.get(id).map(|info| &info.kind) {
                Some(TypeInfoKind::Enum(variants)) => Some(variants.clone()),
                Some(TypeInfoKind::Tagged(variants)) => Some(variants.keys().cloned().collect()),
                _ => None,
            },
            _ => None,
        }
    }

    fn pattern_match_cases(
        &self,
        pattern: &HirPattern,
        ty: &Ty,
        required: &BTreeSet<String>,
    ) -> BTreeSet<String> {
        if self.pattern_is_irrefutable(pattern, ty) {
            return required.clone();
        }
        match &pattern.kind {
            HirPatternKind::Or { patterns } => patterns
                .iter()
                .flat_map(|pattern| self.pattern_match_cases(pattern, ty, required))
                .collect(),
            HirPatternKind::As { pattern, .. } => self.pattern_match_cases(pattern, ty, required),
            HirPatternKind::Literal {
                value: ast::PatternLiteral::Bool { value },
            } if matches!(ty, Ty::Bool) => [value.to_string()].into_iter().collect(),
            HirPatternKind::None { .. } if matches!(ty, Ty::Optional { .. }) => {
                ["None".to_owned()].into_iter().collect()
            }
            HirPatternKind::Some { value } => match ty {
                Ty::Optional { inner } if self.pattern_is_irrefutable(value, inner) => {
                    ["Some".to_owned()].into_iter().collect()
                }
                _ => BTreeSet::new(),
            },
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                if &expected != ty || !required.contains(name) {
                    return BTreeSet::new();
                }
                let covers = match ty {
                    Ty::Nominal(id) => match self.env.types.get(id).map(|info| &info.kind) {
                        Some(TypeInfoKind::Enum(_)) => fields.is_empty(),
                        Some(TypeInfoKind::Tagged(variants)) => {
                            variants.get(name).is_some_and(|defs| {
                                fields.iter().all(|field| {
                                    let Some(info) = defs.get(&field.name) else {
                                        return false;
                                    };
                                    field.pattern.as_ref().is_none_or(|pattern| {
                                        self.pattern_is_irrefutable(pattern, &info.ty)
                                    })
                                })
                            })
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if covers {
                    [name.clone()].into_iter().collect()
                } else {
                    BTreeSet::new()
                }
            }
            _ => BTreeSet::new(),
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
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => {
                        element.as_ref().clone()
                    }
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

fn array_type_has_unknown_length(ty: &Ty) -> bool {
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

fn validate_declaration_array_lengths(
    source: &ast::SourceFile,
    diagnostics: &mut Vec<TypeDiagnostic>,
) {
    fn validate_type(ty: &ast::TypeNode, diagnostics: &mut Vec<TypeDiagnostic>) {
        match &ty.kind {
            ast::TypeKind::Array { element, length } => {
                if eval_const_usize_ast(length).is_none() {
                    diagnostics.push(TypeDiagnostic {
                        span: length.span,
                        code: "type/array-length".into(),
                        message: "array length must be a non-negative compile-time integer".into(),
                    });
                }
                validate_type(element, diagnostics);
            }
            ast::TypeKind::Pointer { inner, .. }
            | ast::TypeKind::Reference { inner, .. }
            | ast::TypeKind::Optional { inner } => validate_type(inner, diagnostics),
            ast::TypeKind::Slice { element, .. } => validate_type(element, diagnostics),
            ast::TypeKind::Result { ok, error } => {
                validate_type(ok, diagnostics);
                validate_type(error, diagnostics);
            }
            ast::TypeKind::Function { params, result }
            | ast::TypeKind::Closure { params, result } => {
                for param in params {
                    validate_type(param, diagnostics);
                }
                validate_type(result, diagnostics);
            }
            ast::TypeKind::Named { .. } => {}
        }
    }

    for declaration in &source.declarations {
        match &declaration.kind.kind {
            DeclKind::Function(function) => {
                for param in &function.params {
                    validate_type(&param.ty, diagnostics);
                }
                if let Some(result) = &function.return_type {
                    validate_type(result, diagnostics);
                }
            }
            DeclKind::Struct(value) => {
                for field in &value.fields {
                    validate_type(&field.ty, diagnostics);
                }
            }
            DeclKind::Tagged(value) => {
                for variant in &value.variants {
                    for field in &variant.fields {
                        validate_type(&field.ty, diagnostics);
                    }
                }
            }
            DeclKind::BitStruct(value) => validate_type(&value.storage, diagnostics),
            DeclKind::Distinct(value) => validate_type(&value.underlying, diagnostics),
            DeclKind::TypeAlias(value) => validate_type(&value.target, diagnostics),
            DeclKind::Impl(value) => {
                for method in &value.methods {
                    for param in &method.function.params {
                        validate_type(&param.ty, diagnostics);
                    }
                    if let Some(result) = &method.function.return_type {
                        validate_type(result, diagnostics);
                    }
                }
            }
            DeclKind::Global(value) => {
                if let Some(ty) = &value.ty {
                    validate_type(ty, diagnostics);
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

fn eval_const_binary(op: BinaryOp, left: i128, right: i128) -> Option<i128> {
    match op {
        BinaryOp::Add => left.checked_add(right),
        BinaryOp::Sub => left.checked_sub(right),
        BinaryOp::Mul => left.checked_mul(right),
        BinaryOp::Div => left.checked_div(right),
        BinaryOp::Rem => left.checked_rem(right),
        BinaryOp::BitAnd => Some(left & right),
        BinaryOp::BitXor => Some(left ^ right),
        BinaryOp::BitOr => Some(left | right),
        BinaryOp::ShiftLeft => u32::try_from(right)
            .ok()
            .and_then(|shift| left.checked_shl(shift)),
        BinaryOp::ShiftRight => u32::try_from(right)
            .ok()
            .and_then(|shift| left.checked_shr(shift)),
        _ => None,
    }
}

fn eval_const_int_ast(expr: &ast::Expr) -> Option<i128> {
    match &expr.kind {
        ast::ExprKind::Integer { text } => parse_integer_value(text),
        ast::ExprKind::Unary { op, value } => match op {
            UnaryOp::Neg => eval_const_int_ast(value)?.checked_neg(),
            UnaryOp::BitNot => Some(!eval_const_int_ast(value)?),
            _ => None,
        },
        ast::ExprKind::Binary { op, left, right } => {
            eval_const_binary(*op, eval_const_int_ast(left)?, eval_const_int_ast(right)?)
        }
        _ => None,
    }
}

fn eval_const_int_hir(expr: &HirExpr) -> Option<i128> {
    match &expr.kind {
        HirExprKind::Integer { text } => parse_integer_value(text),
        HirExprKind::Unary { op, value } => match op {
            UnaryOp::Neg => eval_const_int_hir(value)?.checked_neg(),
            UnaryOp::BitNot => Some(!eval_const_int_hir(value)?),
            _ => None,
        },
        HirExprKind::Binary { op, left, right } => {
            eval_const_binary(*op, eval_const_int_hir(left)?, eval_const_int_hir(right)?)
        }
        _ => None,
    }
}

fn eval_const_usize_ast(expr: &ast::Expr) -> Option<u64> {
    u64::try_from(eval_const_int_ast(expr)?).ok()
}

fn eval_const_usize_hir(expr: &HirExpr) -> Option<u64> {
    u64::try_from(eval_const_int_hir(expr)?).ok()
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
