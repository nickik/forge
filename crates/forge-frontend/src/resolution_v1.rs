use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{self, DeclKind, ExprKind, PatternKind, Span, StmtKind, TypeKind},
    hir::{DefId, HirDiagnostic, HirModule},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct LocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedName {
    Local(LocalId),
    Def(DefId),
    Import(u32),
    BuiltinType,
    BuiltinValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NameUse {
    pub span: Span,
    pub name: String,
    pub resolution: ResolvedName,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HirLocal {
    pub id: LocalId,
    pub name: String,
    pub span: Span,
    pub mutable: bool,
    pub parameter: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedBody {
    pub owner: DefId,
    pub locals: Vec<HirLocal>,
    pub uses: Vec<NameUse>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct BodyResolutionOutput {
    pub bodies: BTreeMap<DefId, ResolvedBody>,
    pub diagnostics: Vec<HirDiagnostic>,
}

pub fn resolve_module_bodies(source: &ast::SourceFile, module: &HirModule) -> BodyResolutionOutput {
    let imports = source
        .imports
        .iter()
        .enumerate()
        .filter_map(|(index, path)| {
            path.segments
                .last()
                .map(|name| (name.clone(), index as u32))
        })
        .collect::<BTreeMap<_, _>>();

    let mut output = BodyResolutionOutput::default();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let owner = DefId(index as u32);
        let mut resolver = Resolver::new(owner, module, &imports, &mut output.diagnostics);
        match &declaration.kind.kind {
            DeclKind::Function(function) => {
                resolver.push_scope();
                for parameter in &function.params {
                    resolver.resolve_type(&parameter.ty);
                    if let Some(default) = &parameter.default {
                        resolver.resolve_expr(default);
                    }
                    resolver.define_local(&parameter.name, parameter.ty.span, false, true);
                }
                if let Some(return_type) = &function.return_type {
                    resolver.resolve_type(return_type);
                }
                resolver.resolve_block(&function.body, false);
                output.bodies.insert(owner, resolver.finish());
            }
            DeclKind::Global(value) => {
                if let Some(ty) = &value.ty {
                    resolver.resolve_type(ty);
                }
                // A global binding is not visible to its own initializer merely because it
                // is being lowered. Module-level lookup still resolves other globals/functions.
                resolver.resolve_expr(&value.value);
                output.bodies.insert(owner, resolver.finish());
            }
            DeclKind::Struct(value) => {
                for field in &value.fields {
                    resolver.resolve_type(&field.ty);
                    if let Some(default) = &field.default {
                        resolver.resolve_expr(default);
                    }
                }
            }
            DeclKind::Enum(value) => {
                for variant in &value.variants {
                    if let Some(expr) = &variant.value {
                        resolver.resolve_expr(expr);
                    }
                }
            }
            DeclKind::Tagged(value) => {
                for variant in &value.variants {
                    for field in &variant.fields {
                        resolver.resolve_type(&field.ty);
                        if let Some(default) = &field.default {
                            resolver.resolve_expr(default);
                        }
                    }
                }
            }
            DeclKind::BitStruct(value) => resolver.resolve_type(&value.storage),
            DeclKind::Distinct(value) => resolver.resolve_type(&value.underlying),
            DeclKind::TypeAlias(value) => resolver.resolve_type(&value.target),
            DeclKind::Impl(value) => {
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
        }
    }
    output
}

struct Resolver<'a, 'd> {
    owner: DefId,
    module: &'a HirModule,
    imports: &'a BTreeMap<String, u32>,
    diagnostics: &'d mut Vec<HirDiagnostic>,
    scopes: Vec<BTreeMap<String, LocalId>>,
    locals: Vec<HirLocal>,
    uses: Vec<NameUse>,
    next_local: u32,
}

impl<'a, 'd> Resolver<'a, 'd> {
    fn new(
        owner: DefId,
        module: &'a HirModule,
        imports: &'a BTreeMap<String, u32>,
        diagnostics: &'d mut Vec<HirDiagnostic>,
    ) -> Self {
        Self {
            owner,
            module,
            imports,
            diagnostics,
            scopes: Vec::new(),
            locals: Vec::new(),
            uses: Vec::new(),
            next_local: 0,
        }
    }

    fn finish(self) -> ResolvedBody {
        ResolvedBody {
            owner: self.owner,
            locals: self.locals,
            uses: self.uses,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_local(&mut self, name: &str, span: Span, mutable: bool, parameter: bool) -> LocalId {
        if self.scopes.is_empty() {
            self.push_scope();
        }
        let scope = self.scopes.last_mut().expect("scope exists");
        if scope.contains_key(name) {
            self.diagnostics.push(HirDiagnostic {
                span,
                message: format!("duplicate local definition `{name}` in the same lexical scope"),
            });
        }
        let id = LocalId(self.next_local);
        self.next_local += 1;
        scope.insert(name.to_owned(), id);
        self.locals.push(HirLocal {
            id,
            name: name.to_owned(),
            span,
            mutable,
            parameter,
        });
        id
    }

    fn resolve_value_name(&mut self, name: &str, span: Span) {
        if let Some(id) = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
        {
            self.record_use(name, span, ResolvedName::Local(id));
            return;
        }
        if let Some(symbol) = self.module.symbols.get(name).and_then(|set| set.value_def) {
            self.record_use(name, span, ResolvedName::Def(symbol));
            return;
        }
        if let Some(index) = self.imports.get(name).copied() {
            self.record_use(name, span, ResolvedName::Import(index));
            return;
        }
        if matches!(name, "Some") {
            self.record_use(name, span, ResolvedName::BuiltinValue);
            return;
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!("unresolved value name `{name}`"),
        });
    }

    fn record_use(&mut self, name: &str, span: Span, resolution: ResolvedName) {
        self.uses.push(NameUse {
            span,
            name: name.to_owned(),
            resolution,
        });
    }

    fn resolve_type_path(&mut self, path: &ast::Path, span: Span) {
        if path.segments.len() == 1 {
            let name = &path.segments[0];
            if builtin_types().contains(name.as_str()) {
                self.record_use(name, span, ResolvedName::BuiltinType);
                return;
            }
            if let Some(def) = self.module.symbols.get(name).and_then(|set| set.type_def) {
                self.record_use(name, span, ResolvedName::Def(def));
                return;
            }
        } else if let Some(first) = path.segments.first() {
            if let Some(index) = self.imports.get(first).copied() {
                self.record_use(first, span, ResolvedName::Import(index));
                return;
            }
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!("unresolved type name `{}`", path.segments.join(".")),
        });
    }

    fn resolve_type(&mut self, ty: &ast::TypeNode) {
        match &ty.kind {
            TypeKind::Named { path } => self.resolve_type_path(path, ty.span),
            TypeKind::Pointer { inner, .. }
            | TypeKind::Reference { inner, .. }
            | TypeKind::Optional { inner }
            | TypeKind::Annotated { inner, .. } => self.resolve_type(inner),
            TypeKind::Slice { element, .. } => self.resolve_type(element),
            TypeKind::Array { element, length } => {
                self.resolve_type(element);
                self.resolve_expr(length);
            }
            TypeKind::Result { ok, error } => {
                self.resolve_type(ok);
                self.resolve_type(error);
            }
            TypeKind::Function { params, result } | TypeKind::Closure { params, result } => {
                for param in params {
                    self.resolve_type(param);
                }
                self.resolve_type(result);
            }
        }
    }

    fn resolve_block(&mut self, block: &ast::Block, create_scope: bool) {
        if create_scope {
            self.push_scope();
        }
        for statement in &block.statements {
            self.resolve_stmt(statement);
        }
        if create_scope {
            self.pop_scope();
        }
    }

    fn resolve_stmt(&mut self, stmt: &ast::Stmt) {
        match &stmt.kind {
            StmtKind::Value(value) => {
                if let Some(ty) = &value.ty {
                    self.resolve_type(ty);
                }
                self.resolve_expr(&value.value);
                let mutable = matches!(value.binding, ast::BindingKind::Var);
                self.bind_pattern(&value.pattern, mutable, false);
            }
            StmtKind::Assignment { target, value } => {
                self.resolve_expr(target);
                self.resolve_expr(value);
            }
            StmtKind::Expr { expr } => self.resolve_expr(expr),
            StmtKind::Return { value, .. } => {
                if let Some(value) = value {
                    self.resolve_expr(value);
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                self.resolve_expr(condition);
                self.resolve_block(then_block, true);
                if let Some(branch) = else_branch {
                    self.resolve_stmt(branch);
                }
            }
            StmtKind::While { condition, body } => {
                self.resolve_expr(condition);
                self.resolve_block(body, true);
            }
            StmtKind::ForC {
                init,
                condition,
                step,
                body,
            } => {
                self.push_scope();
                if let Some(init) = init {
                    match init {
                        ast::ForInit::Value(value) => {
                            if let Some(ty) = &value.ty {
                                self.resolve_type(ty);
                            }
                            self.resolve_expr(&value.value);
                            self.bind_pattern(
                                &value.pattern,
                                matches!(value.binding, ast::BindingKind::Var),
                                false,
                            );
                        }
                        ast::ForInit::Assignment { target, value } => {
                            self.resolve_expr(target);
                            self.resolve_expr(value);
                        }
                        ast::ForInit::Expr(expr) => self.resolve_expr(expr),
                    }
                }
                if let Some(condition) = condition {
                    self.resolve_expr(condition);
                }
                if let Some(step) = step {
                    match step {
                        ast::ForStep::Assignment { target, value } => {
                            self.resolve_expr(target);
                            self.resolve_expr(value);
                        }
                        ast::ForStep::Expr(expr) => self.resolve_expr(expr),
                    }
                }
                self.resolve_block(body, true);
                self.pop_scope();
            }
            StmtKind::ForEach {
                binding,
                pattern,
                iterable,
                body,
            } => {
                self.resolve_expr(iterable);
                self.push_scope();
                self.bind_pattern(pattern, matches!(binding, ast::BindingKind::Var), false);
                self.resolve_block(body, false);
                self.pop_scope();
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Defer { body } => match body {
                ast::DeferBody::Block(block) => self.resolve_block(block, true),
                ast::DeferBody::Expr(expr) => self.resolve_expr(expr),
            },
            StmtKind::Unsafe { body } | StmtKind::Block { block: body } => {
                self.resolve_block(body, true)
            }
            StmtKind::WithContext { overrides, body } => {
                for override_ in overrides {
                    self.resolve_expr(&override_.value);
                }
                self.resolve_block(body, true);
            }
            StmtKind::Select { arms } => {
                for arm in arms {
                    match arm {
                        ast::SelectArm::Receive {
                            channel,
                            pattern,
                            body,
                        } => {
                            self.resolve_expr(channel);
                            self.push_scope();
                            self.bind_pattern(pattern, false, false);
                            self.resolve_block(body, false);
                            self.pop_scope();
                        }
                        ast::SelectArm::Timeout { duration, body } => {
                            self.resolve_expr(duration);
                            self.resolve_block(body, true);
                        }
                    }
                }
            }
        }
    }

    fn bind_pattern(&mut self, pattern: &ast::Pattern, mutable: bool, parameter: bool) {
        match &pattern.kind {
            PatternKind::Binding { name, .. } => {
                self.define_local(name, pattern.span, mutable, parameter);
            }
            PatternKind::Variant { fields, .. } | PatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let Some(pattern) = &field.pattern {
                        self.bind_pattern(pattern, mutable, parameter);
                    } else {
                        self.define_local(&field.name, pattern.span, mutable, parameter);
                    }
                }
            }
            PatternKind::Sequence { items, rest } => {
                for item in items {
                    self.bind_pattern(item, mutable, parameter);
                }
                if let Some(rest) = rest {
                    self.define_local(rest, pattern.span, mutable, parameter);
                }
            }
            PatternKind::Map { entries, .. } => {
                for entry in entries {
                    self.define_local(&entry.binding, pattern.span, mutable, parameter);
                }
            }
            PatternKind::Some { value } => self.bind_pattern(value, mutable, parameter),
            PatternKind::As {
                name,
                pattern: inner,
            } => {
                self.define_local(name, pattern.span, mutable, parameter);
                self.bind_pattern(inner, mutable, parameter);
            }
            PatternKind::Or { patterns } => {
                if let Some(first) = patterns.first() {
                    self.bind_pattern(first, mutable, parameter);
                }
            }
            PatternKind::Wildcard
            | PatternKind::Literal { .. }
            | PatternKind::Range { .. }
            | PatternKind::None { .. } => {}
        }
    }

    fn resolve_expr(&mut self, expr: &ast::Expr) {
        match &expr.kind {
            ExprKind::Path { path } if path.segments.len() == 1 => {
                self.resolve_value_name(&path.segments[0], expr.span)
            }
            ExprKind::Path { path } => {
                if let Some(first) = path.segments.first() {
                    if let Some(index) = self.imports.get(first).copied() {
                        self.record_use(first, expr.span, ResolvedName::Import(index));
                    } else {
                        self.diagnostics.push(HirDiagnostic {
                            span: expr.span,
                            message: format!("unresolved value path `{}`", path.segments.join(".")),
                        });
                    }
                }
            }
            ExprKind::Qualified { namespace, .. } | ExprKind::StructInit { namespace, .. } => {
                self.resolve_type_path(namespace, expr.span);
                if let ExprKind::StructInit { fields, .. } = &expr.kind {
                    for field in fields {
                        self.resolve_expr(&field.value);
                    }
                }
            }
            ExprKind::Array { items } => {
                for item in items {
                    self.resolve_expr(item);
                }
            }
            ExprKind::Unary { value, .. }
            | ExprKind::Try { value }
            | ExprKind::Annotated { value, .. } => self.resolve_expr(value),
            ExprKind::Binary { left, right, .. } => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }
            ExprKind::Call { callee, args } => {
                self.resolve_expr(callee);
                for arg in args {
                    match arg {
                        ast::CallArg::Positional { value } | ast::CallArg::Named { value, .. } => {
                            self.resolve_expr(value)
                        }
                    }
                }
            }
            ExprKind::Index { base, index } => {
                self.resolve_expr(base);
                self.resolve_expr(index);
            }
            ExprKind::Member { base, .. } => self.resolve_expr(base),
            ExprKind::Closure {
                captures,
                params,
                return_type,
                body,
            } => {
                // Capture validation against the enclosing scope is performed by resolving the
                // named capture before introducing the closure-local binding.
                let mut seen = BTreeSet::new();
                self.push_scope();
                for capture in captures {
                    if !seen.insert(capture.name.clone()) {
                        self.diagnostics.push(HirDiagnostic {
                            span: expr.span,
                            message: format!("duplicate closure capture `{}`", capture.name),
                        });
                    }
                    // Captures are local bindings inside the closure. Their source-side target
                    // will be made explicit when closure environment lowering is added.
                    self.define_local(&capture.name, expr.span, capture.mutable, false);
                }
                for param in params {
                    self.resolve_type(&param.ty);
                    self.define_local(&param.name, param.ty.span, false, true);
                }
                if let Some(return_type) = return_type {
                    self.resolve_type(return_type);
                }
                self.resolve_block(body, false);
                self.pop_scope();
            }
            ExprKind::Match { value, arms } => {
                self.resolve_expr(value);
                for arm in arms {
                    self.push_scope();
                    self.bind_pattern(&arm.pattern, false, false);
                    if let Some(guard) = &arm.guard {
                        self.resolve_expr(guard);
                    }
                    match &arm.body {
                        ast::MatchBody::Block(block) => self.resolve_block(block, false),
                        ast::MatchBody::Expr(expr) => self.resolve_expr(expr),
                    }
                    self.pop_scope();
                }
            }
            ExprKind::Integer { .. }
            | ExprKind::Float { .. }
            | ExprKind::Character { .. }
            | ExprKind::String { .. }
            | ExprKind::CString { .. }
            | ExprKind::Bool { .. }
            | ExprKind::None
            | ExprKind::Keyword { .. }
            | ExprKind::ReaderForm { .. } => {}
        }
    }
}

fn builtin_types() -> BTreeSet<&'static str> {
    [
        "bool", "char", "str", "void", "never", "usize", "isize", "u8", "u16", "u32", "u64", "i8",
        "i16", "i32", "i64", "f32", "f64",
    ]
    .into_iter()
    .collect()
}
