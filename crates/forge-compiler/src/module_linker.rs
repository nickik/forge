use std::collections::{BTreeMap, BTreeSet};

use forge_frontend::ast::{
    Block, CallArg, Decl, DeclKind, DeferBody, Expr, ExprKind, ForInit, ForStep, MatchBody, Path,
    Pattern, PatternKind, SelectArm, SourceFile, Stmt, StmtKind, TypeKind, TypeNode, ValueDecl,
};

#[derive(Debug, Clone)]
pub(crate) struct ParsedLibrary {
    pub name: String,
    pub ast: SourceFile,
}

#[derive(Debug, Clone)]
struct Symbol {
    renamed: String,
    public: bool,
}

#[derive(Debug, Clone)]
struct ModulePlan {
    name: String,
    ast: SourceFile,
    imports: BTreeMap<String, String>,
    values: BTreeMap<String, Symbol>,
    types: BTreeMap<String, Symbol>,
    namespace: bool,
}

pub(crate) fn link_modules(
    root: SourceFile,
    libraries: Vec<ParsedLibrary>,
) -> Result<SourceFile, String> {
    let mut plans = BTreeMap::new();
    for library in libraries {
        let module_name = library.ast.module.segments.join(".");
        let alias = library
            .ast
            .module
            .segments
            .last()
            .cloned()
            .ok_or_else(|| format!("library `{}` has an empty module path", library.name))?;
        if library.name != alias && library.name != module_name {
            return Err(format!(
                "--library {}=... provides module `{module_name}`; expected the full module name or import alias `{alias}`",
                library.name
            ));
        }
        if plans.contains_key(&alias) {
            return Err(format!("duplicate library import alias `{alias}`"));
        }
        plans.insert(alias, plan_module(module_name, library.ast, true));
    }

    let root_name = root.module.segments.join(".");
    let root_plan = plan_module(root_name, root, false);
    validate_import_maps(&root_plan, &plans)?;
    for plan in plans.values() {
        validate_import_maps(plan, &plans)?;
    }

    let order = dependency_order(&root_plan, &plans)?;
    let mut declarations = Vec::new();
    for name in order {
        let mut plan = plans
            .get(&name)
            .cloned()
            .ok_or_else(|| format!("internal linker error: missing library `{name}`"))?;
        rewrite_module(&mut plan, &plans)?;
        declarations.extend(plan.ast.declarations);
    }

    let mut root_plan = root_plan;
    rewrite_module(&mut root_plan, &plans)?;
    declarations.extend(root_plan.ast.declarations);

    Ok(SourceFile {
        module: root_plan.ast.module,
        imports: Vec::new(),
        declarations,
    })
}

fn plan_module(name: String, ast: SourceFile, namespace: bool) -> ModulePlan {
    let mut values = BTreeMap::new();
    let mut types = BTreeMap::new();
    for declaration in &ast.declarations {
        match &declaration.kind.kind {
            DeclKind::Function(function) => {
                values.insert(
                    function.name.clone(),
                    Symbol {
                        renamed: rename(&ast.module, &function.name, namespace),
                        public: declaration.kind.public,
                    },
                );
            }
            DeclKind::Struct(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::Enum(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::Tagged(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::BitStruct(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::Distinct(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::TypeAlias(value) => insert_type(
                &mut types,
                &ast.module,
                &value.name,
                declaration.kind.public,
                namespace,
            ),
            DeclKind::Global(value) => {
                let mut names = Vec::new();
                collect_pattern_bindings(&value.pattern, &mut names);
                for name in names {
                    values.insert(
                        name.clone(),
                        Symbol {
                            renamed: rename(&ast.module, &name, namespace),
                            public: declaration.kind.public,
                        },
                    );
                }
            }
            DeclKind::Impl(_) => {}
        }
    }

    let imports = ast
        .imports
        .iter()
        .filter_map(|path| {
            path.segments
                .last()
                .map(|alias| (alias.clone(), alias.clone()))
        })
        .collect();
    ModulePlan {
        name,
        ast,
        imports,
        values,
        types,
        namespace,
    }
}

fn insert_type(
    types: &mut BTreeMap<String, Symbol>,
    module: &Path,
    name: &str,
    public: bool,
    namespace: bool,
) {
    types.insert(
        name.to_owned(),
        Symbol {
            renamed: rename(module, name, namespace),
            public,
        },
    );
}

fn rename(module: &Path, name: &str, namespace: bool) -> String {
    if !namespace {
        return name.to_owned();
    }
    let module = module.segments.join("_");
    format!("__forge_c12c_{module}__{name}")
}

fn validate_import_maps(
    plan: &ModulePlan,
    libraries: &BTreeMap<String, ModulePlan>,
) -> Result<(), String> {
    for import in &plan.ast.imports {
        let alias = import
            .segments
            .last()
            .ok_or_else(|| format!("module `{}` has an empty import", plan.name))?;
        let target = libraries.get(alias).ok_or_else(|| {
            format!(
                "module `{}` imports `{}`, but no --library {alias}=PATH was supplied",
                plan.name,
                import.segments.join(".")
            )
        })?;
        if target.ast.module != *import && target.ast.module.segments.last() != Some(alias) {
            return Err(format!(
                "import `{}` in module `{}` does not match supplied library module `{}`",
                import.segments.join("."),
                plan.name,
                target.ast.module.segments.join(".")
            ));
        }
    }
    Ok(())
}

fn dependency_order(
    root: &ModulePlan,
    libraries: &BTreeMap<String, ModulePlan>,
) -> Result<Vec<String>, String> {
    fn visit(
        name: &str,
        libraries: &BTreeMap<String, ModulePlan>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        order: &mut Vec<String>,
    ) -> Result<(), String> {
        if visited.contains(name) {
            return Ok(());
        }
        if !visiting.insert(name.to_owned()) {
            return Err(format!("library import cycle includes `{name}`"));
        }
        let module = libraries
            .get(name)
            .ok_or_else(|| format!("missing library `{name}`"))?;
        for dependency in module.imports.keys() {
            visit(dependency, libraries, visiting, visited, order)?;
        }
        visiting.remove(name);
        visited.insert(name.to_owned());
        order.push(name.to_owned());
        Ok(())
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for dependency in root.imports.keys() {
        visit(
            dependency,
            libraries,
            &mut visiting,
            &mut visited,
            &mut order,
        )?;
    }
    Ok(order)
}

fn rewrite_module(
    plan: &mut ModulePlan,
    libraries: &BTreeMap<String, ModulePlan>,
) -> Result<(), String> {
    let context = RewriteContext {
        module: &plan.name,
        imports: &plan.imports,
        values: &plan.values,
        types: &plan.types,
        libraries,
    };
    for declaration in &mut plan.ast.declarations {
        rewrite_decl(declaration, &context, plan.namespace)?;
    }
    plan.ast.imports.clear();
    Ok(())
}

struct RewriteContext<'a> {
    module: &'a str,
    imports: &'a BTreeMap<String, String>,
    values: &'a BTreeMap<String, Symbol>,
    types: &'a BTreeMap<String, Symbol>,
    libraries: &'a BTreeMap<String, ModulePlan>,
}

#[derive(Default)]
struct Scopes(Vec<BTreeSet<String>>);

impl Scopes {
    fn push(&mut self) {
        self.0.push(BTreeSet::new());
    }

    fn pop(&mut self) {
        self.0.pop();
    }

    fn bind(&mut self, name: &str) {
        if self.0.is_empty() {
            self.push();
        }
        self.0
            .last_mut()
            .expect("scope exists")
            .insert(name.to_owned());
    }

    fn contains(&self, name: &str) -> bool {
        self.0.iter().rev().any(|scope| scope.contains(name))
    }
}

fn rewrite_decl(
    declaration: &mut Decl,
    context: &RewriteContext<'_>,
    namespace: bool,
) -> Result<(), String> {
    match &mut declaration.kind.kind {
        DeclKind::Function(function) => {
            let old = function.name.clone();
            if namespace {
                function.name = context.values[&old].renamed.clone();
            }
            let mut scopes = Scopes::default();
            scopes.push();
            for parameter in &mut function.params {
                rewrite_type(&mut parameter.ty, context, &scopes)?;
                if let Some(default) = &mut parameter.default {
                    rewrite_expr(default, context, &mut scopes)?;
                }
                scopes.bind(&parameter.name);
            }
            if let Some(result) = &mut function.return_type {
                rewrite_type(result, context, &scopes)?;
            }
            rewrite_block(&mut function.body, context, &mut scopes, false)?;
        }
        DeclKind::Struct(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            let mut scopes = Scopes::default();
            for field in &mut value.fields {
                rewrite_type(&mut field.ty, context, &scopes)?;
                if let Some(default) = &mut field.default {
                    rewrite_expr(default, context, &mut scopes)?;
                }
            }
        }
        DeclKind::Enum(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            let mut scopes = Scopes::default();
            for variant in &mut value.variants {
                if let Some(expr) = &mut variant.value {
                    rewrite_expr(expr, context, &mut scopes)?;
                }
            }
        }
        DeclKind::Tagged(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            let mut scopes = Scopes::default();
            for variant in &mut value.variants {
                for field in &mut variant.fields {
                    rewrite_type(&mut field.ty, context, &scopes)?;
                    if let Some(default) = &mut field.default {
                        rewrite_expr(default, context, &mut scopes)?;
                    }
                }
            }
        }
        DeclKind::BitStruct(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            rewrite_type(&mut value.storage, context, &Scopes::default())?;
        }
        DeclKind::Distinct(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            rewrite_type(&mut value.underlying, context, &Scopes::default())?;
        }
        DeclKind::TypeAlias(value) => {
            let old = value.name.clone();
            if namespace {
                value.name = context.types[&old].renamed.clone();
            }
            rewrite_type(&mut value.target, context, &Scopes::default())?;
        }
        DeclKind::Impl(value) => {
            rewrite_type_path(&mut value.target, context)?;
            for method in &mut value.methods {
                let mut scopes = Scopes::default();
                scopes.push();
                for parameter in &mut method.function.params {
                    rewrite_type(&mut parameter.ty, context, &scopes)?;
                    if let Some(default) = &mut parameter.default {
                        rewrite_expr(default, context, &mut scopes)?;
                    }
                    scopes.bind(&parameter.name);
                }
                if let Some(result) = &mut method.function.return_type {
                    rewrite_type(result, context, &scopes)?;
                }
                rewrite_block(&mut method.function.body, context, &mut scopes, false)?;
            }
        }
        DeclKind::Global(value) => {
            let mut scopes = Scopes::default();
            if let Some(ty) = &mut value.ty {
                rewrite_type(ty, context, &scopes)?;
            }
            rewrite_expr(&mut value.value, context, &mut scopes)?;
            rewrite_pattern_types(&mut value.pattern, context)?;
            if namespace {
                rename_top_pattern(&mut value.pattern, context.values);
            }
        }
    }
    Ok(())
}

fn rewrite_block(
    block: &mut Block,
    context: &RewriteContext<'_>,
    scopes: &mut Scopes,
    create_scope: bool,
) -> Result<(), String> {
    if create_scope {
        scopes.push();
    }
    for statement in &mut block.statements {
        rewrite_stmt(statement, context, scopes)?;
    }
    if create_scope {
        scopes.pop();
    }
    Ok(())
}

fn rewrite_stmt(
    statement: &mut Stmt,
    context: &RewriteContext<'_>,
    scopes: &mut Scopes,
) -> Result<(), String> {
    match &mut statement.kind {
        StmtKind::Value(value) => rewrite_local_value(value, context, scopes)?,
        StmtKind::Assignment { target, value } => {
            rewrite_expr(target, context, scopes)?;
            rewrite_expr(value, context, scopes)?;
        }
        StmtKind::Expr { expr } => rewrite_expr(expr, context, scopes)?,
        StmtKind::Return { value, .. } => {
            if let Some(value) = value {
                rewrite_expr(value, context, scopes)?;
            }
        }
        StmtKind::If {
            condition,
            then_block,
            else_branch,
        } => {
            rewrite_expr(condition, context, scopes)?;
            rewrite_block(then_block, context, scopes, true)?;
            if let Some(branch) = else_branch {
                rewrite_stmt(branch, context, scopes)?;
            }
        }
        StmtKind::While { condition, body } => {
            rewrite_expr(condition, context, scopes)?;
            rewrite_block(body, context, scopes, true)?;
        }
        StmtKind::ForC {
            init,
            condition,
            step,
            body,
        } => {
            scopes.push();
            if let Some(init) = init {
                match init {
                    ForInit::Value(value) => rewrite_local_value(value, context, scopes)?,
                    ForInit::Assignment { target, value } => {
                        rewrite_expr(target, context, scopes)?;
                        rewrite_expr(value, context, scopes)?;
                    }
                    ForInit::Expr(expr) => rewrite_expr(expr, context, scopes)?,
                }
            }
            if let Some(condition) = condition {
                rewrite_expr(condition, context, scopes)?;
            }
            if let Some(step) = step {
                match step {
                    ForStep::Assignment { target, value } => {
                        rewrite_expr(target, context, scopes)?;
                        rewrite_expr(value, context, scopes)?;
                    }
                    ForStep::Expr(expr) => rewrite_expr(expr, context, scopes)?,
                }
            }
            rewrite_block(body, context, scopes, true)?;
            scopes.pop();
        }
        StmtKind::ForEach {
            pattern,
            iterable,
            body,
            ..
        } => {
            rewrite_expr(iterable, context, scopes)?;
            rewrite_pattern_types(pattern, context)?;
            scopes.push();
            bind_pattern(pattern, scopes);
            rewrite_block(body, context, scopes, false)?;
            scopes.pop();
        }
        StmtKind::Break | StmtKind::Continue => {}
        StmtKind::Defer { body } => match body {
            DeferBody::Block(block) => rewrite_block(block, context, scopes, true)?,
            DeferBody::Expr(expr) => rewrite_expr(expr, context, scopes)?,
        },
        StmtKind::Unsafe { body } | StmtKind::Block { block: body } => {
            rewrite_block(body, context, scopes, true)?
        }
        StmtKind::WithContext { overrides, body } => {
            for override_ in overrides {
                rewrite_expr(&mut override_.value, context, scopes)?;
            }
            rewrite_block(body, context, scopes, true)?;
        }
        StmtKind::Select { arms } => {
            for arm in arms {
                match arm {
                    SelectArm::Receive {
                        channel,
                        pattern,
                        body,
                    } => {
                        rewrite_expr(channel, context, scopes)?;
                        rewrite_pattern_types(pattern, context)?;
                        scopes.push();
                        bind_pattern(pattern, scopes);
                        rewrite_block(body, context, scopes, false)?;
                        scopes.pop();
                    }
                    SelectArm::Timeout { duration, body } => {
                        rewrite_expr(duration, context, scopes)?;
                        rewrite_block(body, context, scopes, true)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn rewrite_local_value(
    value: &mut ValueDecl,
    context: &RewriteContext<'_>,
    scopes: &mut Scopes,
) -> Result<(), String> {
    if let Some(ty) = &mut value.ty {
        rewrite_type(ty, context, scopes)?;
    }
    rewrite_expr(&mut value.value, context, scopes)?;
    rewrite_pattern_types(&mut value.pattern, context)?;
    bind_pattern(&value.pattern, scopes);
    Ok(())
}

fn rewrite_expr(
    expr: &mut Expr,
    context: &RewriteContext<'_>,
    scopes: &mut Scopes,
) -> Result<(), String> {
    if let ExprKind::Member { base, name } = &expr.kind {
        if let ExprKind::Path { path } = &base.kind {
            if path.segments.len() == 1 {
                let alias = &path.segments[0];
                if context.imports.contains_key(alias) {
                    let symbol = imported_symbol(context, alias, name, false)?;
                    expr.kind = ExprKind::Path {
                        path: Path {
                            segments: vec![symbol],
                        },
                    };
                    return Ok(());
                }
            }
        }
    }

    match &mut expr.kind {
        ExprKind::Path { path } => rewrite_value_path(path, context, scopes)?,
        ExprKind::Qualified { namespace, .. } => rewrite_type_path(namespace, context)?,
        ExprKind::Array { items } => {
            for item in items {
                rewrite_expr(item, context, scopes)?;
            }
        }
        ExprKind::StructInit {
            namespace, fields, ..
        } => {
            rewrite_type_path(namespace, context)?;
            for field in fields {
                rewrite_expr(&mut field.value, context, scopes)?;
            }
        }
        ExprKind::Unary { value, .. } | ExprKind::Try { value } => {
            rewrite_expr(value, context, scopes)?
        }
        ExprKind::Binary { left, right, .. } => {
            rewrite_expr(left, context, scopes)?;
            rewrite_expr(right, context, scopes)?;
        }
        ExprKind::Call { callee, args } => {
            rewrite_expr(callee, context, scopes)?;
            for arg in args {
                match arg {
                    CallArg::Positional { value } | CallArg::Named { value, .. } => {
                        rewrite_expr(value, context, scopes)?
                    }
                }
            }
        }
        ExprKind::Index { base, index } => {
            rewrite_expr(base, context, scopes)?;
            rewrite_expr(index, context, scopes)?;
        }
        ExprKind::Member { base, .. } => rewrite_expr(base, context, scopes)?,
        ExprKind::Closure {
            params,
            return_type,
            body,
            ..
        } => {
            scopes.push();
            for parameter in params {
                rewrite_type(&mut parameter.ty, context, scopes)?;
                if let Some(default) = &mut parameter.default {
                    rewrite_expr(default, context, scopes)?;
                }
                scopes.bind(&parameter.name);
            }
            if let Some(result) = return_type {
                rewrite_type(result, context, scopes)?;
            }
            rewrite_block(body, context, scopes, false)?;
            scopes.pop();
        }
        ExprKind::Match { value, arms } => {
            rewrite_expr(value, context, scopes)?;
            for arm in arms {
                rewrite_pattern_types(&mut arm.pattern, context)?;
                scopes.push();
                bind_pattern(&arm.pattern, scopes);
                if let Some(guard) = &mut arm.guard {
                    rewrite_expr(guard, context, scopes)?;
                }
                match &mut arm.body {
                    MatchBody::Block(block) => rewrite_block(block, context, scopes, false)?,
                    MatchBody::Expr(expr) => rewrite_expr(expr, context, scopes)?,
                }
                scopes.pop();
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
        | ExprKind::Context { .. }
        | ExprKind::ReaderForm { .. } => {}
    }
    Ok(())
}

fn rewrite_value_path(
    path: &mut Path,
    context: &RewriteContext<'_>,
    scopes: &Scopes,
) -> Result<(), String> {
    if path.segments.len() == 1 {
        let name = &path.segments[0];
        if !scopes.contains(name) {
            if let Some(symbol) = context.values.get(name) {
                path.segments = vec![symbol.renamed.clone()];
            }
        }
        return Ok(());
    }

    let alias = path.segments[0].clone();
    if context.imports.contains_key(&alias) {
        if path.segments.len() != 2 {
            return Err(format!(
                "C12c value import `{}` in module `{}` must name one exported definition",
                path.segments.join("."),
                context.module
            ));
        }
        let name = path.segments[1].clone();
        let symbol = imported_symbol(context, &alias, &name, false)?;
        path.segments = vec![symbol];
    }
    Ok(())
}

fn rewrite_type(
    ty: &mut TypeNode,
    context: &RewriteContext<'_>,
    scopes: &Scopes,
) -> Result<(), String> {
    match &mut ty.kind {
        TypeKind::Named { path } => rewrite_type_path(path, context)?,
        TypeKind::Pointer { inner, .. }
        | TypeKind::Reference { inner, .. }
        | TypeKind::Optional { inner } => rewrite_type(inner, context, scopes)?,
        TypeKind::Slice { element, .. } => rewrite_type(element, context, scopes)?,
        TypeKind::Array { element, length } => {
            rewrite_type(element, context, scopes)?;
            let mut local_scopes = Scopes(scopes.0.clone());
            rewrite_expr(length, context, &mut local_scopes)?;
        }
        TypeKind::Result { ok, error } => {
            rewrite_type(ok, context, scopes)?;
            rewrite_type(error, context, scopes)?;
        }
        TypeKind::Function { params, result } | TypeKind::Closure { params, result } => {
            for param in params {
                rewrite_type(param, context, scopes)?;
            }
            rewrite_type(result, context, scopes)?;
        }
    }
    Ok(())
}

fn rewrite_type_path(path: &mut Path, context: &RewriteContext<'_>) -> Result<(), String> {
    if path.segments.len() == 1 {
        if let Some(symbol) = context.types.get(&path.segments[0]) {
            path.segments = vec![symbol.renamed.clone()];
        }
        return Ok(());
    }

    let alias = path.segments[0].clone();
    if context.imports.contains_key(&alias) {
        if path.segments.len() != 2 {
            return Err(format!(
                "C12c type import `{}` in module `{}` must name one exported type",
                path.segments.join("."),
                context.module
            ));
        }
        let name = path.segments[1].clone();
        let symbol = imported_symbol(context, &alias, &name, true)?;
        path.segments = vec![symbol];
    }
    Ok(())
}

fn imported_symbol(
    context: &RewriteContext<'_>,
    alias: &str,
    name: &str,
    type_namespace: bool,
) -> Result<String, String> {
    let module = context.libraries.get(alias).ok_or_else(|| {
        format!(
            "module `{}` imports unknown library `{alias}`",
            context.module
        )
    })?;
    let table = if type_namespace {
        &module.types
    } else {
        &module.values
    };
    let symbol = table.get(name).ok_or_else(|| {
        format!(
            "module `{}` imports `{alias}.{name}`, but library `{alias}` has no such {}",
            context.module,
            if type_namespace { "type" } else { "value" }
        )
    })?;
    if !symbol.public {
        return Err(format!(
            "module `{}` cannot import private {} `{alias}.{name}`",
            context.module,
            if type_namespace { "type" } else { "value" }
        ));
    }
    Ok(symbol.renamed.clone())
}

fn rewrite_pattern_types(
    pattern: &mut Pattern,
    context: &RewriteContext<'_>,
) -> Result<(), String> {
    match &mut pattern.kind {
        PatternKind::Variant {
            namespace, fields, ..
        } => {
            rewrite_type_path(namespace, context)?;
            for field in fields {
                if let Some(pattern) = &mut field.pattern {
                    rewrite_pattern_types(pattern, context)?;
                }
            }
        }
        PatternKind::Struct { path, fields } => {
            rewrite_type_path(path, context)?;
            for field in fields {
                if let Some(pattern) = &mut field.pattern {
                    rewrite_pattern_types(pattern, context)?;
                }
            }
        }
        PatternKind::Sequence { items, .. } | PatternKind::Or { patterns: items } => {
            for item in items {
                rewrite_pattern_types(item, context)?;
            }
        }
        PatternKind::Some { value } | PatternKind::Ok { value } | PatternKind::Err { value } => {
            rewrite_pattern_types(value, context)?
        }
        PatternKind::As { pattern, .. } => rewrite_pattern_types(pattern, context)?,
        PatternKind::Wildcard
        | PatternKind::Binding { .. }
        | PatternKind::Literal { .. }
        | PatternKind::Range { .. }
        | PatternKind::None { .. }
        | PatternKind::Map { .. } => {}
    }
    Ok(())
}

fn collect_pattern_bindings(pattern: &Pattern, out: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Binding { name, .. } => out.push(name.clone()),
        PatternKind::Variant { fields, .. } | PatternKind::Struct { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &field.pattern {
                    collect_pattern_bindings(pattern, out);
                } else {
                    out.push(field.name.clone());
                }
            }
        }
        PatternKind::Sequence { items, rest } => {
            for item in items {
                collect_pattern_bindings(item, out);
            }
            if let Some(rest) = rest {
                out.push(rest.clone());
            }
        }
        PatternKind::Map { entries, .. } => {
            out.extend(entries.iter().map(|entry| entry.binding.clone()));
        }
        PatternKind::Some { value } | PatternKind::Ok { value } | PatternKind::Err { value } => {
            collect_pattern_bindings(value, out)
        }
        PatternKind::Or { patterns } => {
            if let Some(first) = patterns.first() {
                collect_pattern_bindings(first, out);
            }
        }
        PatternKind::As { name, pattern } => {
            out.push(name.clone());
            collect_pattern_bindings(pattern, out);
        }
        PatternKind::Wildcard
        | PatternKind::Literal { .. }
        | PatternKind::Range { .. }
        | PatternKind::None { .. } => {}
    }
}

fn bind_pattern(pattern: &Pattern, scopes: &mut Scopes) {
    let mut names = Vec::new();
    collect_pattern_bindings(pattern, &mut names);
    for name in names {
        scopes.bind(&name);
    }
}

fn rename_top_pattern(pattern: &mut Pattern, values: &BTreeMap<String, Symbol>) {
    match &mut pattern.kind {
        PatternKind::Binding { name, .. } => {
            if let Some(symbol) = values.get(name) {
                *name = symbol.renamed.clone();
            }
        }
        PatternKind::Variant { fields, .. } | PatternKind::Struct { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &mut field.pattern {
                    rename_top_pattern(pattern, values);
                } else if let Some(symbol) = values.get(&field.name) {
                    field.name = symbol.renamed.clone();
                }
            }
        }
        PatternKind::Sequence { items, rest } => {
            for item in items {
                rename_top_pattern(item, values);
            }
            if let Some(rest) = rest {
                if let Some(symbol) = values.get(rest) {
                    *rest = symbol.renamed.clone();
                }
            }
        }
        PatternKind::Map { entries, .. } => {
            for entry in entries {
                if let Some(symbol) = values.get(&entry.binding) {
                    entry.binding = symbol.renamed.clone();
                }
            }
        }
        PatternKind::Some { value } | PatternKind::Ok { value } | PatternKind::Err { value } => {
            rename_top_pattern(value, values)
        }
        PatternKind::Or { patterns } => {
            for pattern in patterns {
                rename_top_pattern(pattern, values);
            }
        }
        PatternKind::As { name, pattern } => {
            if let Some(symbol) = values.get(name) {
                *name = symbol.renamed.clone();
            }
            rename_top_pattern(pattern, values);
        }
        PatternKind::Wildcard
        | PatternKind::Literal { .. }
        | PatternKind::Range { .. }
        | PatternKind::None { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_frontend::{lower_module, parse_source};

    fn parse(source: &str) -> SourceFile {
        let parsed = parse_source(source);
        assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
        parsed.ast.expect("ast")
    }

    #[test]
    fn imported_public_function_is_resolved_without_textual_inclusion() {
        let root = parse(
            r#"
            module app;
            import math;
            fn main() -> i32 { return math.answer(); }
            "#,
        );
        let library = parse(
            r#"
            module math;
            pub fn answer() -> i32 { return 42; }
            "#,
        );
        let linked = link_modules(
            root,
            vec![ParsedLibrary {
                name: "math".into(),
                ast: library,
            }],
        )
        .expect("link modules");
        assert!(linked.imports.is_empty());
        let hir = lower_module(&linked);
        assert!(hir.diagnostics.is_empty(), "{:?}", hir.diagnostics);
        assert!(hir.module.symbols.contains_key("__forge_c12c_math__answer"));
        assert!(hir.module.symbols.contains_key("main"));
    }

    #[test]
    fn imported_private_definition_is_rejected() {
        let root = parse(
            r#"
            module app;
            import math;
            fn main() -> i32 { return math.secret(); }
            "#,
        );
        let library = parse(
            r#"
            module math;
            fn secret() -> i32 { return 1; }
            "#,
        );
        let error = link_modules(
            root,
            vec![ParsedLibrary {
                name: "math".into(),
                ast: library,
            }],
        )
        .unwrap_err();
        assert!(error.contains("private value `math.secret`"), "{error}");
    }
}
