from pathlib import Path


def read(path):
    return Path(path).read_text()


def write(path, text):
    Path(path).write_text(text)


def replace_once(path, old, new):
    text = read(path)
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:120]!r}")
    text = text.replace(old, new, 1)
    write(path, text)


def replace_all(path, old, new):
    text = read(path)
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:120]!r}")
    text = text.replace(old, new)
    write(path, text)


# ---------------------------------------------------------------------------
# HIR: first-class method DefIds + generic metadata query API.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    '''    ImplMethod {
        owner: DefId,
        name: String,
    },
''',
    '',
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    '''    pub symbols: BTreeMap<String, SymbolSet>,
    pub metadata: MetadataTable,
}
''',
    '''    pub symbols: BTreeMap<String, SymbolSet>,
    pub methods: Vec<HirMethod>,
    pub metadata: MetadataTable,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirMethod {
    pub id: DefId,
    pub impl_owner: DefId,
    pub target: ast::Path,
    pub name: String,
}

pub trait MetadataTableExt {
    fn for_target(&self, target: &MetadataTarget) -> &[ast::Metadata];
    fn named<'a>(&'a self, target: &MetadataTarget, name: &'a str)
        -> impl Iterator<Item = &'a ast::Metadata>;
}

impl MetadataTableExt for MetadataTable {
    fn for_target(&self, target: &MetadataTarget) -> &[ast::Metadata] {
        self.get(target).map(Vec::as_slice).unwrap_or(&[])
    }

    fn named<'a>(
        &'a self,
        target: &MetadataTarget,
        name: &'a str,
    ) -> impl Iterator<Item = &'a ast::Metadata> {
        self.for_target(target)
            .iter()
            .filter(move |metadata| metadata.name.as_deref() == Some(name))
    }
}
''',
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    '''    Impl { target: ast::Path },
''',
    '''    Impl {
        target: ast::Path,
        methods: Vec<DefId>,
    },
''',
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    '''        symbols: BTreeMap::new(),
        metadata: BTreeMap::new(),
    };
    let mut diagnostics = Vec::new();

    for (index, declaration) in source.declarations.iter().enumerate() {
''',
    '''        symbols: BTreeMap::new(),
        methods: Vec::new(),
        metadata: BTreeMap::new(),
    };
    let mut diagnostics = Vec::new();
    let mut next_method_id = source.declarations.len() as u32;

    for (index, declaration) in source.declarations.iter().enumerate() {
''',
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    '''            DeclKind::Impl(value) => {
                for method in &value.methods {
                    record_metadata(
                        &mut module.metadata,
                        MetadataTarget::ImplMethod {
                            owner: id,
                            name: method.function.name.clone(),
                        },
                        &method.metadata,
                    );
                }
                HirItemKind::Impl {
                    target: value.target.clone(),
                }
            }
''',
    '''            DeclKind::Impl(value) => {
                let mut methods = Vec::with_capacity(value.methods.len());
                for method in &value.methods {
                    let method_id = DefId(next_method_id);
                    next_method_id += 1;
                    record_metadata(
                        &mut module.metadata,
                        MetadataTarget::Item { owner: method_id },
                        &method.metadata,
                    );
                    module.methods.push(HirMethod {
                        id: method_id,
                        impl_owner: id,
                        target: value.target.clone(),
                        name: method.function.name.clone(),
                    });
                    methods.push(method_id);
                }
                HirItemKind::Impl {
                    target: value.target.clone(),
                    methods,
                }
            }
''',
)

# ---------------------------------------------------------------------------
# Body HIR: lower method bodies and declaration-time expressions.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    '''    pub params: Vec<(LocalId, HirType)>,
    pub return_type: Option<HirType>,
    pub locals: Vec<HirLocalDecl>,
''',
    '''    pub params: Vec<(LocalId, HirType)>,
    pub param_defaults: BTreeMap<LocalId, HirExpr>,
    pub return_type: Option<HirType>,
    pub locals: Vec<HirLocalDecl>,
''',
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    '''pub struct BodyHirOutput {
    pub functions: BTreeMap<DefId, HirBody>,
    pub globals: BTreeMap<DefId, HirGlobalBody>,
    pub diagnostics: Vec<HirDiagnostic>,
}
''',
    '''pub struct BodyHirOutput {
    pub functions: BTreeMap<DefId, HirBody>,
    pub globals: BTreeMap<DefId, HirGlobalBody>,
    pub field_defaults: Vec<HirTypedDeclExpr>,
    pub enum_values: Vec<HirEnumValueExpr>,
    pub diagnostics: Vec<HirDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirTypedDeclExpr {
    pub owner: DefId,
    pub label: String,
    pub expected: HirType,
    pub value: HirExpr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirEnumValueExpr {
    pub owner: DefId,
    pub variant: String,
    pub value: HirExpr,
}
''',
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    '''                let mut params = Vec::new();
                for parameter in &function.params {
                    let ty = lowerer.lower_type(&parameter.ty);
                    let id = lowerer.define_local(parameter.ty.span, false, true, &parameter.name);
                    params.push((id, ty));
                }
''',
    '''                let mut params = Vec::new();
                let mut param_defaults = BTreeMap::new();
                for parameter in &function.params {
                    let ty = lowerer.lower_type(&parameter.ty);
                    let default = parameter.default.as_ref().map(|value| lowerer.lower_expr(value));
                    let id = lowerer.define_local(parameter.ty.span, false, true, &parameter.name);
                    if let Some(default) = default {
                        param_defaults.insert(id, default);
                    }
                    params.push((id, ty));
                }
''',
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    '''                        owner,
                        params,
                        return_type,
''',
    '''                        owner,
                        params,
                        param_defaults,
                        return_type,
''',
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    '''            ast::DeclKind::Global(value) => {
                let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                let ty = value.ty.as_ref().map(|ty| lowerer.lower_type(ty));
                let expr = lowerer.lower_expr(&value.value);
                output.globals.insert(
                    owner,
                    HirGlobalBody {
                        owner,
                        ty,
                        value: expr,
                    },
                );
            }
            _ => {}
''',
    '''            ast::DeclKind::Global(value) => {
                let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                let ty = value.ty.as_ref().map(|ty| lowerer.lower_type(ty));
                let expr = lowerer.lower_expr(&value.value);
                output.globals.insert(
                    owner,
                    HirGlobalBody {
                        owner,
                        ty,
                        value: expr,
                    },
                );
            }
            ast::DeclKind::Struct(value) => {
                for field in &value.fields {
                    if let Some(default) = &field.default {
                        let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                        output.field_defaults.push(HirTypedDeclExpr {
                            owner,
                            label: field.name.clone(),
                            expected: lowerer.lower_type(&field.ty),
                            value: lowerer.lower_expr(default),
                        });
                    }
                }
            }
            ast::DeclKind::Tagged(value) => {
                for variant in &value.variants {
                    for field in &variant.fields {
                        if let Some(default) = &field.default {
                            let mut lowerer =
                                Lowerer::new(module, &imports, &mut output.diagnostics);
                            output.field_defaults.push(HirTypedDeclExpr {
                                owner,
                                label: format!("{}::{}", variant.name, field.name),
                                expected: lowerer.lower_type(&field.ty),
                                value: lowerer.lower_expr(default),
                            });
                        }
                    }
                }
            }
            ast::DeclKind::Enum(value) => {
                for variant in &value.variants {
                    if let Some(explicit) = &variant.value {
                        let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                        output.enum_values.push(HirEnumValueExpr {
                            owner,
                            variant: variant.name.clone(),
                            value: lowerer.lower_expr(explicit),
                        });
                    }
                }
            }
            ast::DeclKind::Impl(value) => {
                let method_defs = module
                    .methods
                    .iter()
                    .filter(|method| method.impl_owner == owner)
                    .collect::<Vec<_>>();
                for (method, method_def) in value.methods.iter().zip(method_defs) {
                    let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                    lowerer.push_scope();
                    let mut params = Vec::new();
                    let mut param_defaults = BTreeMap::new();
                    for parameter in &method.function.params {
                        let ty = lowerer.lower_type(&parameter.ty);
                        let default = parameter
                            .default
                            .as_ref()
                            .map(|value| lowerer.lower_expr(value));
                        let id = lowerer.define_local(
                            parameter.ty.span,
                            false,
                            true,
                            &parameter.name,
                        );
                        if let Some(default) = default {
                            param_defaults.insert(id, default);
                        }
                        params.push((id, ty));
                    }
                    let return_type = method
                        .function
                        .return_type
                        .as_ref()
                        .map(|ty| lowerer.lower_type(ty));
                    let block = lowerer.lower_block(&method.function.body, false);
                    output.functions.insert(
                        method_def.id,
                        HirBody {
                            owner: method_def.id,
                            params,
                            param_defaults,
                            return_type,
                            locals: lowerer.locals,
                            block,
                        },
                    );
                }
            }
            _ => {}
''',
)

# ---------------------------------------------------------------------------
# Resolution: method bodies use method DefIds rather than the impl container ID.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/resolution_v1.rs",
    '''            DeclKind::Impl(value) => {
                resolver.resolve_type_path(&value.target, declaration.span);
                for method in &value.methods {
                    resolver.push_scope();
                    for parameter in &method.function.params {
                        resolver.resolve_type(&parameter.ty);
                        resolver.define_local(&parameter.name, parameter.ty.span, false, true);
                    }
                    if let Some(return_type) = &method.function.return_type {
                        resolver.resolve_type(return_type);
                    }
                    resolver.resolve_block(&method.function.body, false);
                    resolver.pop_scope();
                }
                output.bodies.insert(owner, resolver.finish());
            }
''',
    '''            DeclKind::Impl(value) => {
                resolver.resolve_type_path(&value.target, declaration.span);
                drop(resolver);
                let method_defs = module
                    .methods
                    .iter()
                    .filter(|method| method.impl_owner == owner)
                    .collect::<Vec<_>>();
                for (method, method_def) in value.methods.iter().zip(method_defs) {
                    let mut method_resolver = Resolver::new(
                        method_def.id,
                        module,
                        &imports,
                        &qualified_only_variants,
                        &mut output.diagnostics,
                    );
                    method_resolver.push_scope();
                    for parameter in &method.function.params {
                        method_resolver.resolve_type(&parameter.ty);
                        if let Some(default) = &parameter.default {
                            method_resolver.resolve_expr(default);
                        }
                        method_resolver.define_local(
                            &parameter.name,
                            parameter.ty.span,
                            false,
                            true,
                        );
                    }
                    if let Some(return_type) = &method.function.return_type {
                        method_resolver.resolve_type(return_type);
                    }
                    method_resolver.resolve_block(&method.function.body, false);
                    method_resolver.pop_scope();
                    output
                        .bodies
                        .insert(method_def.id, method_resolver.finish());
                }
            }
''',
)

# ---------------------------------------------------------------------------
# Type system: preserve array length, method lookup, declaration checks,
# resolved calls, exhaustiveness.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    Array {
        element: Box<Ty>,
    },
''',
    '''    Array {
        element: Box<Ty>,
        length: Option<u64>,
    },
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''pub enum TypedExprKind {
    Source { hir: HirExpr },
    OptionalPromote { value: Box<TypedExpr> },
}
''',
    '''pub enum TypedExprKind {
    Source { hir: HirExpr },
    ResolvedCall {
        target: DefId,
        method: bool,
        hir: HirExpr,
    },
    OptionalPromote { value: Box<TypedExpr> },
}
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''struct ModuleTypeEnv {
    types: BTreeMap<DefId, TypeInfo>,
    functions: BTreeMap<DefId, FunctionSig>,
}
''',
    '''struct ModuleTypeEnv {
    types: BTreeMap<DefId, TypeInfo>,
    functions: BTreeMap<DefId, FunctionSig>,
    methods: BTreeMap<(DefId, String), DefId>,
}
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            types: BTreeMap::new(),
            functions: BTreeMap::new(),
        };
''',
    '''            types: BTreeMap::new(),
            functions: BTreeMap::new(),
            methods: BTreeMap::new(),
        };
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            ast::TypeKind::Array { element, .. } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
            },
''',
    '''            ast::TypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_ast_type(element, module)),
                length: eval_const_usize_ast(length),
            },
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            HirTypeKind::Array { element, .. } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
            },
''',
    '''            HirTypeKind::Array { element, length } => Ty::Array {
                element: Box::new(self.lower_hir_type(element)),
                length: eval_const_usize_hir(length),
            },
''',
)
# Array literal keeps exact N.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''                Ty::Array {
                    element: Box::new(element),
                }
''',
    '''                Ty::Array {
                    element: Box::new(element),
                    length: Some(items.len() as u64),
                }
''',
)
# Common array pattern matches.
replace_all(
    "crates/forge-frontend/src/typecheck_v1.rs",
    'Ty::Array { element } | Ty::Slice { element, .. }',
    'Ty::Array { element, .. } | Ty::Slice { element, .. }',
)
replace_all(
    "crates/forge-frontend/src/typecheck_v1.rs",
    'Ty::Array { element }',
    'Ty::Array { element, .. }',
)

# Add method signatures to ModuleTypeEnv after top-level function signatures.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        for (index, declaration) in source.declarations.iter().enumerate() {
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
''',
    '''        for (index, declaration) in source.declarations.iter().enumerate() {
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
            let Some(target_id) = module.symbols.get(target_name).and_then(|set| set.type_def) else {
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
''',
)
# lookup_method helper.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    fn distinct_underlying(&self, id: DefId) -> Option<&Ty> {
''',
    '''    fn lookup_method(&self, ty: &Ty, name: &str) -> Option<(DefId, &FunctionSig)> {
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
''',
)

# Type-check defaults before body and declaration expressions globally.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    let env = ModuleTypeEnv::build(source, module);

    for (owner, body) in &bodies.functions {
''',
    '''    validate_declaration_array_lengths(source, &mut output.diagnostics);
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
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        for (local, ty) in &body.params {
            checker.local_types.insert(*local, env.lower_hir_type(ty));
        }
''',
    '''        for (local, ty) in &body.params {
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
''',
)

# check_expr: track resolved call target in typed HIR.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut ty = match &expr.kind {
''',
    '''    fn check_expr(&mut self, expr: &HirExpr, expected: Option<&Ty>) -> Ty {
        let mut resolved_call: Option<(DefId, bool)> = None;
        let mut ty = match &expr.kind {
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            HirExprKind::Call { callee, args } => self.check_call(expr.span, callee, args),
''',
    '''            HirExprKind::Call { callee, args } => {
                let (result, target) = self.check_call(expr.span, callee, args);
                resolved_call = target;
                result
            }
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        self.expressions.push(TypedExpr {
            span: expr.span,
            ty: ty.clone(),
            kind: TypedExprKind::Source { hir: expr.clone() },
        });
''',
    '''        let kind = resolved_call
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
''',
)

# check_call implementation with method lookup and DefId propagation.
start = read("crates/forge-frontend/src/typecheck_v1.rs")
old_start = '''    fn check_call(&mut self, span: Span, callee: &HirExpr, args: &[HirCallArg]) -> Ty {\n'''
old_end = '''    fn check_function_args(&mut self, span: Span, sig: &FunctionSig, args: &[HirCallArg]) {\n'''
if old_start not in start or old_end not in start:
    raise SystemExit("check_call markers missing")
a = start.index(old_start)
b = start.index(old_end, a)
new_check_call = '''    fn check_call(
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
            Ty::Reference {
                mutable,
                inner,
            } => {
                let compatible = match actual {
                    Ty::Reference {
                        mutable: actual_mutable,
                        inner: actual_inner,
                    } => {
                        actual_inner.as_ref() == inner.as_ref()
                            && (!*mutable || *actual_mutable)
                    }
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

'''
start = start[:a] + new_check_call + start[b:]
write("crates/forge-frontend/src/typecheck_v1.rs", start)

# Sequence irrefutability can now use retained array length.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            // Length-sensitive sequence declarations require fixed-array length to be
            // retained in Ty. Until then, sequence declarations cannot be proven safe.
            HirPatternKind::Sequence { .. }
            | HirPatternKind::Map { .. }
''',
    '''            HirPatternKind::Sequence { items, rest } => match ty {
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
''',
)

# Exhaustiveness check after match arms.
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''                    }
                }
                result
            }
            HirExprKind::Error => Ty::Error,
''',
    '''                    }
                }
                self.check_match_exhaustiveness(expr.span, &matched, arms);
                result
            }
            HirExprKind::Error => Ty::Error,
''',
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    fn check_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
''',
    '''    fn check_match_exhaustiveness(
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
            Ty::Bool => Some(["false".to_owned(), "true".to_owned()].into_iter().collect()),
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
            } if matches!(ty, Ty::Bool) => {
                [value.to_string()].into_iter().collect()
            }
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
                        Some(TypeInfoKind::Tagged(variants)) => variants.get(name).is_some_and(|defs| {
                            fields.iter().all(|field| {
                                let Some(info) = defs.get(&field.name) else {
                                    return false;
                                };
                                field.pattern.as_ref().is_none_or(|pattern| {
                                    self.pattern_is_irrefutable(pattern, &info.ty)
                                })
                            })
                        }),
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
''',
)

# ---------------------------------------------------------------------------
# Const integer helpers and array length validation.
# ---------------------------------------------------------------------------
insert_marker = '''fn arg_value(arg: &HirCallArg) -> &HirExpr {\n'''
text = read("crates/forge-frontend/src/typecheck_v1.rs")
if insert_marker not in text:
    raise SystemExit("arg_value marker missing")
helpers = r'''fn array_type_has_unknown_length(ty: &Ty) -> bool {
    match ty {
        Ty::Array { element, length } => length.is_none() || array_type_has_unknown_length(element),
        Ty::Pointer { inner, .. }
        | Ty::Reference { inner, .. }
        | Ty::Optional { inner } => array_type_has_unknown_length(inner),
        Ty::Slice { element, .. } => array_type_has_unknown_length(element),
        Ty::Result { ok, error } => {
            array_type_has_unknown_length(ok) || array_type_has_unknown_length(error)
        }
        Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
            params.iter().any(array_type_has_unknown_length) || array_type_has_unknown_length(result)
        }
        _ => false,
    }
}

fn validate_declaration_array_lengths(source: &ast::SourceFile, diagnostics: &mut Vec<TypeDiagnostic>) {
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
        BinaryOp::ShiftLeft => u32::try_from(right).ok().and_then(|shift| left.checked_shl(shift)),
        BinaryOp::ShiftRight => u32::try_from(right).ok().and_then(|shift| left.checked_shr(shift)),
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

'''
text = text.replace(insert_marker, helpers + insert_marker, 1)
write("crates/forge-frontend/src/typecheck_v1.rs", text)

# ---------------------------------------------------------------------------
# Public exports.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/lib.rs",
    '''    lower_module, DefId, HirDiagnostic, HirModule, HirOutput, MetadataTable, MetadataTarget,
    Namespace,
''',
    '''    lower_module, DefId, HirDiagnostic, HirMethod, HirModule, HirOutput, MetadataTable,
    MetadataTableExt, MetadataTarget, Namespace,
''',
)

# ---------------------------------------------------------------------------
# Tests.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/tests/hir.rs",
    '''    let method = output
        .module
        .metadata
        .get(&MetadataTarget::ImplMethod {
            owner: DefId(2),
            name: "get".into(),
        })
        .expect("impl method metadata");
''',
    '''    assert_eq!(output.module.methods.len(), 1);
    assert_eq!(output.module.methods[0].id, DefId(3));
    let method = output
        .module
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(3) })
        .expect("impl method metadata");
''',
)

with Path("crates/forge-frontend/tests/typecheck.rs").open("a") as f:
    f.write(r'''

#[test]
fn fixed_array_length_is_preserved_and_used_for_irrefutable_patterns() {
    let output = check(
        r#"
        module test.array_length;
        fn head(pair: [u32; 2]) -> u32 {
            val [left, right] = pair;
            return left + right;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|ty| matches!(
        ty,
        Ty::Array {
            length: Some(2),
            ..
        }
    )));
}

#[test]
fn declaration_defaults_are_checked_early() {
    let output = check(
        r#"
        module test.bad_default;
        struct Config { retries: u8 = true; }
        nfn connect(port: u16 = false) -> bool { return true; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.code == "type/declaration-default")
            .count()
            >= 2,
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn method_calls_resolve_to_method_defids() {
    let output = check(
        r#"
        module test.method_defid;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
            fn set(self: &mut Point, x: i32) -> void { self.x = x; }
        }
        fn main() -> i32 {
            var p = Point{x: 1};
            p.set(2);
            return p.get();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let mut method_targets = output
        .functions
        .values()
        .flat_map(|body| body.expressions.iter())
        .filter_map(|expr| match &expr.kind {
            forge_frontend::TypedExprKind::ResolvedCall {
                target,
                method: true,
                ..
            } => Some(*target),
            _ => None,
        })
        .collect::<Vec<_>>();
    method_targets.sort();
    method_targets.dedup();
    assert_eq!(method_targets.len(), 2, "{method_targets:?}");
}

#[test]
fn mutable_method_rejects_immutable_receiver() {
    let output = check(
        r#"
        module test.method_mutability;
        struct Point { x: i32; }
        impl Point {
            fn set(self: &mut Point, x: i32) -> void { self.x = x; }
        }
        fn main() -> i32 {
            val p = Point{x: 1};
            p.set(2);
            return 0;
        }
        "#,
    );
    assert!(
        has(&output, "method/immutable-receiver"),
        "{:?}",
        output.diagnostics
    );
}

#[test]
fn finite_matches_are_checked_for_exhaustiveness() {
    let output = check(
        r#"
        module test.exhaustive;
        enum Color { Red, Green }
        fn bad(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
            };
        }
        "#,
    );
    assert!(
        has(&output, "match/non-exhaustive"),
        "{:?}",
        output.diagnostics
    );

    let ok = check(
        r#"
        module test.exhaustive_ok;
        enum Color { Red, Green }
        fn good(color: Color) -> i32 {
            return match (color) {
                Color::Red => 1,
                Color::Green => 2,
            };
        }
        "#,
    );
    assert!(ok.diagnostics.is_empty(), "{:?}", ok.diagnostics);
}
''')

# ---------------------------------------------------------------------------
# Conformance additions.
# ---------------------------------------------------------------------------
Path("examples/conformance/negative/17-declaration-default-type.fg").write_text('''module examples.conformance.bad_default_type;\n\nstruct Config { retries: u8 = true; }\n\nfn main() -> i32 { return 0; }\n''')
Path("examples/conformance/negative/18-non-exhaustive-match.fg").write_text('''module examples.conformance.non_exhaustive_match;\n\nenum Color { Red, Green }\n\nfn read(color: Color) -> i32 {\n    return match (color) {\n        Color::Red => 1,\n    };\n}\n''')
replace_once(
    "examples/conformance/suite.fdn",
    '''    {:path #path "negative/16-refutable-destructuring-binding.fg" :kind :negative :expect :pattern/refutable-binding}
''',
    '''    {:path #path "negative/16-refutable-destructuring-binding.fg" :kind :negative :expect :pattern/refutable-binding}
    {:path #path "negative/17-declaration-default-type.fg" :kind :negative :expect :type/declaration-default}
    {:path #path "negative/18-non-exhaustive-match.fg" :kind :negative :expect :match/non-exhaustive}
''',
)

# ---------------------------------------------------------------------------
# V1 docs: constraints deferred; array lengths semantic; method DefIds;
# collection map patterns explicitly deferred; bitstruct proposal documented as open.
# ---------------------------------------------------------------------------
spec = read("docs/forge-v1-spec.md")
start = spec.index("## 15. Range-constrained types")
end = spec.index("## 16. Fixed arrays", start)
spec = spec[:start] + '''## 15. Constrained types\n\nConstrained/refined scalar types are deferred beyond Forge v1. A v1 type alias remains transparent and does not impose runtime range checks. Metadata named `@range` may be carried as ordinary tool metadata, but it has no standardized v1 type-system meaning.\n\n''' + spec[end:]
spec = spec.replace("@range(...)\n", "")
write("docs/forge-v1-spec.md", spec)

syntax = read("docs/forge-v1-syntax-decisions.md")
syntax = syntax.replace('''@range(0..=100)\ntype Percentage = u8;\n\n''', '''type Percentage = u8;\n\n''')
syntax = syntax.replace('''type Percentage = u8 @range(0..=100);\n''', '''type Percentage = u8 @range(0..=100); // rejected: postfix metadata\n''')
write("docs/forge-v1-syntax-decisions.md", syntax)

# Positive fixtures must not imply constrained types are a v1 feature.
for path in [
    "examples/conformance/parse/05-resolved-v1-syntax.fg",
    "examples/conformance/parse/07-declarations-types.fg",
    "examples/conformance/parse/22-metadata-forms.fg",
]:
    text = read(path)
    text = text.replace("@range(0..=100)\ntype Percentage = u8;", "type Percentage = u8;")
    text = text.replace("@range(0..=100)\ntype Percent = u8;", "type Percent = u8;")
    write(path, text)

architecture = read("docs/compiler-architecture.md")
architecture += '''\n\n## Semantic cleanup notes\n\n- Fixed-array length is part of semantic `Ty::Array` and must survive into typed HIR/FIR.\n- Impl methods receive real `DefId`s; typed calls resolve directly to the method/function `DefId`.\n- Declaration-owned default expressions are typechecked before FIR and accumulate diagnostics with ordinary body errors.\n- Exhaustiveness for finite built-in/nominal sums (`bool`, optionals, enums, tagged unions) belongs in semantic type checking because this is the first layer with both resolved type identity and typed patterns. More advanced pattern-matrix optimization can remain a later pass.\n- Map/collection pattern typing is deliberately deferred until Forge has a collection-pattern protocol. Preserve the HIR pattern shape; do not invent `Unknown`-driven semantics in FIR.\n- Metadata remains one target-keyed table. Use the generic metadata query API instead of adding one field per attribute.\n\n### Bitstruct v1 proposal (not yet normative)\n\nKeep bitstructs simple: restrict storage to unsigned fixed-width integers; map each field to the smallest ordinary unsigned integer type that can hold its declared width; never create source-level 3-bit/5-bit integer types; compile-time-known out-of-range writes are errors and dynamic writes are checked rather than truncated. Field ordering and bit numbering still need an explicit language decision before implementation.\n'''
write("docs/compiler-architecture.md", architecture)

print("semantic cleanup patch applied")
