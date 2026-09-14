use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{self, BinaryOp, DeclKind, Span, UnaryOp},
    body_hir::{
        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern,
        HirPatternKind, HirStmt, HirStmtKind, HirType, HirTypeKind, HirTypeRef,
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
    Duration,
    ContextSlot(ContextSlot),
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
#[serde(tag = "const", rename_all = "snake_case")]
pub enum ConstValue {
    Integer { value: i128 },
    Bool { value: bool },
    Char { value: char },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BitStructFieldInfo {
    pub offset: u32,
    pub width: u32,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BitStructInfo {
    pub storage: Ty,
    pub storage_bits: u32,
    pub fields: BTreeMap<String, BitStructFieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedGlobal {
    pub owner: DefId,
    pub binding: ast::BindingKind,
    pub ty: Ty,
    pub initializer: HirExpr,
    pub expressions: Vec<TypedExpr>,
    pub unsafe_expressions: BTreeSet<ExprId>,
    pub call_plans: BTreeMap<ExprId, ResolvedCallPlan>,
    pub closure_plans: BTreeMap<ExprId, TypedClosurePlan>,
    pub match_plans: BTreeMap<ExprId, TypedMatchPlan>,
    pub select_receives: BTreeMap<ExprId, TypedSelectReceive>,
}

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
    pub(crate) fn from_name(name: &str) -> Option<Self> {
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
#[serde(tag = "pattern_kind", rename_all = "snake_case")]
pub enum TypedPatternKind {
    Wildcard,
    Binding {
        local: LocalId,
    },
    Literal {
        value: ast::PatternLiteral,
    },
    Range {
        start: ast::PatternLiteral,
        end: ast::PatternLiteral,
        inclusive: bool,
    },
    Variant {
        name: String,
        fields: Vec<TypedPatternField>,
    },
    None,
    Some {
        value: Box<TypedPattern>,
    },
    Struct {
        fields: Vec<TypedPatternField>,
    },
    Sequence {
        items: Vec<TypedPattern>,
        rest: Option<LocalId>,
    },
    Or {
        patterns: Vec<TypedPattern>,
    },
    As {
        local: LocalId,
        pattern: Box<TypedPattern>,
    },
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDiagnostic {
    pub span: Span,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedExpr {
    pub id: ExprId,
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
        receiver: Option<ResolvedReceiver>,
        argument_parameters: Vec<usize>,
        hir: HirExpr,
    },
    ResolvedTry {
        source_error: Ty,
        target_error: Ty,
        hir: HirExpr,
    },
    OptionalPromote {
        source_type: Ty,
        inner: Box<TypedExprKind>,
        hir: HirExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedReceiver {
    Value,
    SharedReference,
    MutableReference,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedBody {
    pub owner: DefId,
    pub params: Vec<(LocalId, Ty)>,
    pub return_type: Ty,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
    pub unsafe_expressions: BTreeSet<ExprId>,
    pub call_plans: BTreeMap<ExprId, ResolvedCallPlan>,
    pub closure_plans: BTreeMap<ExprId, TypedClosurePlan>,
    pub match_plans: BTreeMap<ExprId, TypedMatchPlan>,
    pub select_receives: BTreeMap<ExprId, TypedSelectReceive>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct TypeCheckOutput {
    pub functions: BTreeMap<DefId, TypedBody>,
    pub globals: BTreeMap<DefId, TypedGlobal>,
    pub global_types: BTreeMap<DefId, Ty>,
    pub bitstructs: BTreeMap<DefId, BitStructInfo>,
    pub constants: BTreeMap<DefId, ConstValue>,
    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,
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
    default: Option<HirExpr>,
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
    BitStruct(BitStructInfo),
    Nominal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberLookup {
    Field(Ty),
    MissingField,
    Unsupported,
    Unknown,
}

fn collect_bitstruct_info(
    source: &ast::SourceFile,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> BTreeMap<DefId, BitStructInfo> {
    let mut result = BTreeMap::new();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let DeclKind::BitStruct(bitstruct) = &declaration.kind.kind else {
            continue;
        };
        let owner = DefId(index as u32);
        let (storage, storage_bits) = match &bitstruct.storage.kind {
            ast::TypeKind::Named { path } if path.segments.len() == 1 => match path.segments[0]
                .as_str()
            {
                "u8" => (
                    Ty::Int {
                        signed: false,
                        width: IntWidth::W8,
                    },
                    8,
                ),
                "u16" => (
                    Ty::Int {
                        signed: false,
                        width: IntWidth::W16,
                    },
                    16,
                ),
                "u32" => (
                    Ty::Int {
                        signed: false,
                        width: IntWidth::W32,
                    },
                    32,
                ),
                "u64" => (
                    Ty::Int {
                        signed: false,
                        width: IntWidth::W64,
                    },
                    64,
                ),
                _ => {
                    diagnostics.push(TypeDiagnostic {
                        span: bitstruct.storage.span,
                        code: "bitstruct/storage".into(),
                        message: "bitstruct storage must be exactly u8, u16, u32, or u64".into(),
                    });
                    continue;
                }
            },
            _ => {
                diagnostics.push(TypeDiagnostic {
                    span: bitstruct.storage.span,
                    code: "bitstruct/storage".into(),
                    message: "bitstruct storage must be exactly u8, u16, u32, or u64".into(),
                });
                continue;
            }
        };
        let mut offset = 0u32;
        let mut fields = BTreeMap::new();
        for field in &bitstruct.fields {
            if field.width == 0 || field.width > storage_bits {
                diagnostics.push(TypeDiagnostic {
                    span: declaration.span,
                    code: "bitstruct/field-width".into(),
                    message: format!(
                        "bitstruct field `{}` has invalid width {}; width must be in 1..={storage_bits}",
                        field.name, field.width
                    ),
                });
                continue;
            }
            let ty = match field.width {
                1 => Ty::Bool,
                2..=8 => Ty::Int {
                    signed: false,
                    width: IntWidth::W8,
                },
                9..=16 => Ty::Int {
                    signed: false,
                    width: IntWidth::W16,
                },
                17..=32 => Ty::Int {
                    signed: false,
                    width: IntWidth::W32,
                },
                _ => Ty::Int {
                    signed: false,
                    width: IntWidth::W64,
                },
            };
            fields.insert(
                field.name.clone(),
                BitStructFieldInfo {
                    offset,
                    width: field.width,
                    ty,
                },
            );
            offset = offset.saturating_add(field.width);
        }
        if offset != storage_bits {
            diagnostics.push(TypeDiagnostic {
                span: declaration.span,
                code: "bitstruct/size".into(),
                message: format!(
                    "bitstruct fields total {offset} bits but storage requires exactly {storage_bits}; spell unused bits as reserved fields"
                ),
            });
        }
        result.insert(
            owner,
            BitStructInfo {
                storage,
                storage_bits,
                fields,
            },
        );
    }
    result
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
    let (constant_values, constant_diagnostics) =
        ConstEvaluator::new(source, bodies).evaluate_all();
    output.constants = constant_values.clone();
    output.diagnostics.extend(constant_diagnostics);
    validate_declaration_array_lengths(source, module, &constant_values, &mut output.diagnostics);
    let bitstructs = collect_bitstruct_info(source, &mut output.diagnostics);
    output.bitstructs = bitstructs.clone();
    let env = ModuleTypeEnv::build(source, module, bodies, &constant_values, &bitstructs);

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
                format!(
                    "enum value must be a compile-time integer expression: {}",
                    error.message
                ),
            ),
        }
    }

    for (owner, body) in &bodies.functions {
        let expected_return = env
            .functions
            .get(owner)
            .map(|sig| sig.result.clone())
            .unwrap_or(Ty::Unknown);
        let mut checker = BodyChecker::new(&env, expected_return.clone(), &mut output.diagnostics);
        let mut typed_params = Vec::with_capacity(body.params.len());
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
            checker.local_types.insert(*local, param_ty.clone());
            typed_params.push((*local, param_ty));
        }
        checker.check_block(&body.block);
        output.functions.insert(
            *owner,
            TypedBody {
                owner: *owner,
                params: typed_params,
                return_type: expected_return,
                local_types: checker.local_types,
                local_constants: checker.local_constants,
                expressions: checker.expressions,
                unsafe_expressions: checker.unsafe_expressions,
                call_plans: checker.call_plans,
                closure_plans: checker.closure_plans,
                match_plans: checker.match_plans,
                select_receives: checker.select_receives,
            },
        );
    }

    for (owner, global) in &bodies.globals {
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
        output.globals.insert(
            *owner,
            TypedGlobal {
                owner: *owner,
                binding: global.binding,
                ty,
                initializer: global.value.clone(),
                expressions: checker.expressions,
                unsafe_expressions: checker.unsafe_expressions,
                call_plans: checker.call_plans,
                closure_plans: checker.closure_plans,
                match_plans: checker.match_plans,
                select_receives: checker.select_receives,
            },
        );
    }

    output
}

struct ModuleTypeEnv {
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
        bodies: &BodyHirOutput,
        constants: &BTreeMap<DefId, ConstValue>,
        bitstructs: &BTreeMap<DefId, BitStructInfo>,
    ) -> Self {
        let mut env = Self {
            types: BTreeMap::new(),
            functions: BTreeMap::new(),
            methods: BTreeMap::new(),
            globals: BTreeMap::new(),
            constants: constants.clone(),
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
                DeclKind::BitStruct(_) => {
                    if let Some(info) = bitstructs.get(&id) {
                        env.types.insert(
                            id,
                            TypeInfo {
                                kind: TypeInfoKind::BitStruct(info.clone()),
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        for (index, declaration) in source.declarations.iter().enumerate() {
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
                let body = bodies.functions.get(&id);
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
                            default,
                        }
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
                let body = bodies.functions.get(&method_def.id);
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
                            default,
                        }
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
                length: eval_const_ast_resolved(length, module, &self.constants)
                    .ok()
                    .and_then(const_value_to_u64),
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
            | Some(TypeInfoKind::BitStruct(_))
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
                Some(TypeInfoKind::BitStruct(info)) => info
                    .fields
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

#[derive(Debug, Clone)]
struct ResolvedCallInfo {
    target: DefId,
    method: bool,
    receiver: Option<ResolvedReceiver>,
    argument_parameters: Vec<usize>,
    arguments: Vec<ResolvedCallArgument>,
}

struct BodyChecker<'a, 'd> {
    env: &'a ModuleTypeEnv,
    expected_return: Ty,
    local_types: BTreeMap<LocalId, Ty>,
    local_constants: BTreeMap<LocalId, ConstValue>,
    mutable_locals: BTreeSet<LocalId>,
    expressions: Vec<TypedExpr>,
    unsafe_depth: u32,
    unsafe_expressions: BTreeSet<ExprId>,
    call_plans: BTreeMap<ExprId, ResolvedCallPlan>,
    closure_plans: BTreeMap<ExprId, TypedClosurePlan>,
    match_plans: BTreeMap<ExprId, TypedMatchPlan>,
    select_receives: BTreeMap<ExprId, TypedSelectReceive>,
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
            local_constants: BTreeMap::new(),
            mutable_locals: BTreeSet::new(),
            expressions: Vec::new(),
            unsafe_depth: 0,
            unsafe_expressions: BTreeSet::new(),
            call_plans: BTreeMap::new(),
            closure_plans: BTreeMap::new(),
            match_plans: BTreeMap::new(),
            select_receives: BTreeMap::new(),
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
                        Err(error) => self.diagnostic(error.span, "const/eval", error.message),
                    }
                }
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
            }
            HirStmtKind::Assignment { target, value } => {
                self.check_assignment_target(target);
                let target_ty = self.check_expr(target, None);
                let value_ty = self.check_expr(value, Some(&target_ty));
                self.require_assignable(value.span, &target_ty, &value_ty, "type/mismatch");
                self.check_bitstruct_write(target, value);
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
            HirStmtKind::DeferBlock { block } | HirStmtKind::Block { block } => {
                self.check_block(block)
            }
            HirStmtKind::Unsafe { block } => {
                self.unsafe_depth += 1;
                self.check_block(block);
                self.unsafe_depth -= 1;
            }
            HirStmtKind::WithContext { overrides, body } => {
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
                    if matches!(
                        actual,
                        Ty::Unknown
                            | Ty::Error
                            | Ty::IntLiteral
                            | Ty::FloatLiteral
                            | Ty::NoneLiteral
                    ) {
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
                            if let Some((recv_target, sig)) =
                                self.env.lookup_method(&channel_ty, "receive")
                            {
                                let sig = sig.clone();
                                if sig.params.len() != 1 {
                                    self.diagnostic(
                                        channel.span,
                                        "select/channel-protocol",
                                        "select channel `receive` method must take only its receiver",
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
                                        "type {channel_ty:?} does not provide the required `receive` method"
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
            HirStmtKind::Break | HirStmtKind::Continue => {}
        }
    }

    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut resolved_call: Option<ResolvedCallInfo> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;
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
            HirExprKind::Keyword { .. } => Ty::Unknown,
            HirExprKind::ReaderForm { tag, .. } if tag == "duration" => Ty::Duration,
            HirExprKind::ReaderForm { .. } => Ty::Unknown,
            HirExprKind::Context { name } => match ContextSlot::from_name(name) {
                Some(slot) => Ty::ContextSlot(slot),
                None => {
                    self.diagnostic(
                        expr.span,
                        "context/unknown-slot",
                        format!("unknown core context slot `{name}`"),
                    );
                    Ty::Error
                }
            },
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
                if matches!(op, UnaryOp::Deref) && matches!(&v, Ty::Pointer { .. }) {
                    if self.unsafe_depth == 0 {
                        self.diagnostic(
                            expr.span,
                            "unsafe/required",
                            "raw pointer dereference requires an `unsafe` block",
                        );
                    } else {
                        self.unsafe_expressions.insert(expr.id);
                    }
                }
                self.check_unary(expr.span, *op, v)
            }
            HirExprKind::Binary { op, left, right } => {
                self.check_binary(expr.span, *op, left, right, expected)
            }
            HirExprKind::Call { callee, args } => {
                let (result, call) = self.check_call(expr.span, callee, args);
                resolved_call = call;
                result
            }
            HirExprKind::TypeCall { target, args } => {
                self.check_type_call(expr.id, expr.span, target, args)
            }
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
            HirExprKind::Closure {
                captures,
                params,
                return_type,
                body,
            } => {
                let outer_types = self.local_types.clone();
                let mut typed_captures = Vec::with_capacity(captures.len());
                for capture in captures {
                    let source_ty = match capture.source {
                        ResolvedName::Local(id) => {
                            outer_types.get(&id).cloned().unwrap_or(Ty::Error)
                        }
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
                let patterns = arms
                    .iter()
                    .map(|arm| self.resolve_typed_pattern(&arm.pattern, &matched))
                    .collect();
                self.match_plans.insert(
                    expr.id,
                    TypedMatchPlan {
                        scrutinee_type: matched.clone(),
                        patterns,
                    },
                );
                result
            }
            HirExprKind::Error => Ty::Error,
        };

        let mut optional_promotion = None;
        if let Some(expected) = expected {
            if let Ty::Optional { inner } = expected {
                if matches!(ty, Ty::NoneLiteral) {
                    ty = expected.clone();
                } else if !matches!(ty, Ty::Optional { .. }) && self.is_assignable(inner, &ty) {
                    let source_type = match &ty {
                        Ty::IntLiteral | Ty::FloatLiteral => inner.as_ref().clone(),
                        other => other.clone(),
                    };
                    optional_promotion = Some(source_type);
                    ty = expected.clone();
                }
            } else if matches!(ty, Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral)
                && self.is_assignable(expected, &ty)
            {
                ty = expected.clone();
            }
        }
        if let Some(call) = resolved_call.as_ref() {
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
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                hir: expr.clone(),
            }
        } else if let Some(call) = resolved_call {
            TypedExprKind::ResolvedCall {
                target: call.target,
                method: call.method,
                receiver: call.receiver,
                argument_parameters: call.argument_parameters,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
        let kind = if let Some(source_type) = optional_promotion {
            TypedExprKind::OptionalPromote {
                source_type,
                inner: Box::new(base_kind),
                hir: expr.clone(),
            }
        } else {
            base_kind
        };
        self.expressions.push(TypedExpr {
            id: expr.id,
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

    fn check_bitstruct_write(&mut self, target: &HirExpr, value: &HirExpr) {
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
        let Some(id) = nominal else {
            return;
        };
        let Some(TypeInfoKind::BitStruct(info)) = self.env.types.get(&id).map(|info| &info.kind)
        else {
            return;
        };
        let Some(field) = info.fields.get(name) else {
            return;
        };
        if field.width == 1 {
            return;
        }
        if let Ok(ConstValue::Integer { value: integer }) =
            eval_const_hir_with_locals(value, &self.env.constants, &self.local_constants)
        {
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
            ResolvedName::Def(id) => {
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
    ) -> (Ty, Option<ResolvedCallInfo>) {
        if let HirExprKind::Member { base, name } = &callee.kind {
            let receiver_ty = self.check_expr(base, None);
            if let Some((method_id, sig)) = self.env.lookup_method(&receiver_ty, name) {
                let sig = sig.clone();
                self.check_method_receiver(base, &receiver_ty, &sig);
                let receiver = match sig.params.first().map(|param| &param.ty) {
                    Some(Ty::Reference { mutable: true, .. }) => {
                        Some(ResolvedReceiver::MutableReference)
                    }
                    Some(Ty::Reference { mutable: false, .. }) => {
                        Some(ResolvedReceiver::SharedReference)
                    }
                    Some(_) => Some(ResolvedReceiver::Value),
                    None => None,
                };
                let reduced = FunctionSig {
                    params: sig.params.iter().skip(1).cloned().collect(),
                    result: sig.result.clone(),
                    named_arguments: sig.named_arguments,
                };
                let (argument_parameters, arguments) =
                    self.check_function_args(span, &reduced, args);
                return (
                    sig.result,
                    Some(ResolvedCallInfo {
                        target: method_id,
                        method: true,
                        receiver,
                        argument_parameters,
                        arguments,
                    }),
                );
            }
        }
        if let HirExprKind::Name { reference } = &callee.kind {
            if let ResolvedName::Def(id) = reference.root {
                if let Some(sig) = self.env.functions.get(&id).cloned() {
                    let (argument_parameters, arguments) =
                        self.check_function_args(span, &sig, args);
                    return (
                        sig.result,
                        Some(ResolvedCallInfo {
                            target: id,
                            method: false,
                            receiver: None,
                            argument_parameters,
                            arguments,
                        }),
                    );
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

    fn check_function_args(
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
                normalized.push(ResolvedCallArgument::Provided {
                    parameter,
                    argument,
                });
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

    fn check_type_call(
        &mut self,
        expr_id: ExprId,
        span: Span,
        target: &HirTypeRef,
        args: &[HirCallArg],
    ) -> Ty {
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
        let raw_conversion = matches!(
            (&target_ty, &source),
            (Ty::Pointer { .. }, Ty::Int { .. } | Ty::Byte)
                | (Ty::Int { .. } | Ty::Byte, Ty::Pointer { .. })
                | (Ty::Pointer { .. }, Ty::Pointer { .. })
        );
        if raw_conversion {
            if self.unsafe_depth == 0 {
                self.diagnostic(
                    span,
                    "unsafe/required",
                    "raw pointer conversion requires an `unsafe` block",
                );
            } else {
                self.unsafe_expressions.insert(expr_id);
            }
        }
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

    fn resolve_typed_pattern(&mut self, pattern: &HirPattern, ty: &Ty) -> TypedPattern {
        let kind = match &pattern.kind {
            HirPatternKind::Wildcard => TypedPatternKind::Wildcard,
            HirPatternKind::Binding { local, .. } => TypedPatternKind::Binding { local: *local },
            HirPatternKind::Literal { value } => TypedPatternKind::Literal {
                value: value.clone(),
            },
            HirPatternKind::Range {
                start,
                end,
                inclusive,
            } => TypedPatternKind::Range {
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
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
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
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => {
                        element.as_ref().clone()
                    }
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

    fn check_match_exhaustiveness(
        &mut self,
        span: Span,
        ty: &Ty,
        arms: &[crate::body_hir::HirMatchArm],
    ) {
        let Some(required) = self.finite_match_cases(ty) else {
            if !arms
                .iter()
                .any(|arm| arm.guard.is_none() && self.pattern_is_irrefutable(&arm.pattern, ty))
            {
                self.diagnostic(
                    span,
                    "match/non-exhaustive",
                    "non-exhaustive match over an open domain; add an unguarded wildcard/irrefutable arm",
                );
            }
            return;
        };
        let mut covered = BTreeSet::new();
        for arm in arms {
            let arm_cases = self.pattern_match_cases(&arm.pattern, ty, &required);
            if !arm_cases.is_empty() && arm_cases.is_subset(&covered) {
                self.diagnostic(
                    arm.pattern.span,
                    "match/unreachable-arm",
                    "match arm is unreachable because earlier unguarded arms cover all of its cases",
                );
                continue;
            }
            if arm.guard.is_none() {
                covered.extend(arm_cases);
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
        if matches!(pattern.kind, HirPatternKind::Map { .. }) {
            self.diagnostic(
                pattern.span,
                "pattern/map-deferred",
                "map/collection patterns are reserved but not part of Forge v1 until a typed collection-pattern protocol is defined",
            );
            return;
        }
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
        if matches!(
            (target, source),
            (Ty::Pointer { .. }, Ty::Int { .. } | Ty::Byte)
                | (Ty::Int { .. } | Ty::Byte, Ty::Pointer { .. })
                | (Ty::Pointer { .. }, Ty::Pointer { .. })
        ) {
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
                let begin = self
                    .stack
                    .iter()
                    .position(|candidate| *candidate == id)
                    .unwrap_or(0);
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
                message:
                    "Forge v1 constant evaluation requires a single-name global `const` binding"
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
                if matches!(op, BinaryOp::LogicalAnd)
                    && matches!(left, ConstValue::Bool { value: false })
                {
                    return Ok(ConstValue::Bool { value: false });
                }
                if matches!(op, BinaryOp::LogicalOr)
                    && matches!(left, ConstValue::Bool { value: true })
                {
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
            if matches!(op, BinaryOp::LogicalAnd)
                && matches!(left, ConstValue::Bool { value: false })
            {
                return Ok(ConstValue::Bool { value: false });
            }
            if matches!(op, BinaryOp::LogicalOr) && matches!(left, ConstValue::Bool { value: true })
            {
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
    eval_const_hir_with_locals(expr, constants, &BTreeMap::new())
}

fn eval_const_hir_with_locals(
    expr: &HirExpr,
    constants: &BTreeMap<DefId, ConstValue>,
    local_constants: &BTreeMap<LocalId, ConstValue>,
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
            ResolvedName::Local(id) => {
                local_constants
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| ConstEvalError {
                        span: expr.span,
                        message: format!("local {id:?} is not a compile-time `const`"),
                    })
            }
            _ => Err(ConstEvalError {
                span: expr.span,
                message: "constant expression may reference only compile-time `const` bindings"
                    .into(),
            }),
        },
        HirExprKind::Unary { op, value } => apply_const_unary(
            expr.span,
            *op,
            eval_const_hir_with_locals(value, constants, local_constants)?,
        ),
        HirExprKind::Binary { op, left, right } => {
            let left = eval_const_hir_with_locals(left, constants, local_constants)?;
            if matches!(op, BinaryOp::LogicalAnd)
                && matches!(left, ConstValue::Bool { value: false })
            {
                return Ok(ConstValue::Bool { value: false });
            }
            if matches!(op, BinaryOp::LogicalOr) && matches!(left, ConstValue::Bool { value: true })
            {
                return Ok(ConstValue::Bool { value: true });
            }
            let right = eval_const_hir_with_locals(right, constants, local_constants)?;
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
                BinaryOp::Add => left
                    .checked_add(right)
                    .map_or_else(|| Err(const_arithmetic_error(span)), integer),
                BinaryOp::Sub => left
                    .checked_sub(right)
                    .map_or_else(|| Err(const_arithmetic_error(span)), integer),
                BinaryOp::Mul => left
                    .checked_mul(right)
                    .map_or_else(|| Err(const_arithmetic_error(span)), integer),
                BinaryOp::Div => left
                    .checked_div(right)
                    .map_or_else(|| Err(const_arithmetic_error(span)), integer),
                BinaryOp::Rem => left
                    .checked_rem(right)
                    .map_or_else(|| Err(const_arithmetic_error(span)), integer),
                BinaryOp::BitAnd => integer(left & right),
                BinaryOp::BitXor => integer(left ^ right),
                BinaryOp::BitOr => integer(left | right),
                BinaryOp::ShiftLeft => {
                    let shift = u32::try_from(right).map_err(|_| ConstEvalError {
                        span,
                        message: "constant shift count is outside the supported range".into(),
                    })?;
                    left.checked_shl(shift)
                        .map_or_else(|| Err(const_arithmetic_error(span)), integer)
                }
                BinaryOp::ShiftRight => {
                    let shift = u32::try_from(right).map_err(|_| ConstEvalError {
                        span,
                        message: "constant shift count is outside the supported range".into(),
                    })?;
                    left.checked_shr(shift)
                        .map_or_else(|| Err(const_arithmetic_error(span)), integer)
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
