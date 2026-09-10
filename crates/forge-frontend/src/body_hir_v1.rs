use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    ast::{self, ExprKind, PatternKind, Span, StmtKind, TypeKind},
    hir::{DefId, HirDiagnostic, HirModule},
    resolution::{LocalId, ResolvedName},
};

pub type HirExpr = HirNode<HirExprKind>;
pub type HirStmt = HirNode<HirStmtKind>;
pub type HirPattern = HirNode<HirPatternKind>;
pub type HirType = HirNode<HirTypeKind>;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirNode<T> {
    pub span: Span,
    pub kind: T,
}

impl<T> HirNode<T> {
    fn new(span: Span, kind: T) -> Self {
        Self { span, kind }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirBody {
    pub owner: DefId,
    pub params: Vec<(LocalId, HirType)>,
    pub return_type: Option<HirType>,
    pub locals: Vec<HirLocalDecl>,
    pub block: HirBlock,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirGlobalBody {
    pub owner: DefId,
    pub ty: Option<HirType>,
    pub value: HirExpr,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct BodyHirOutput {
    pub functions: BTreeMap<DefId, HirBody>,
    pub globals: BTreeMap<DefId, HirGlobalBody>,
    pub diagnostics: Vec<HirDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirLocalDecl {
    pub id: LocalId,
    pub span: Span,
    pub mutable: bool,
    pub parameter: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirBlock {
    pub span: Span,
    pub statements: Vec<HirStmt>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "stmt", rename_all = "snake_case")]
pub enum HirStmtKind {
    Value {
        mutable: bool,
        pattern: HirPattern,
        ty: Option<HirType>,
        value: HirExpr,
    },
    Assignment {
        target: HirExpr,
        value: HirExpr,
    },
    Expr {
        expr: HirExpr,
    },
    Return {
        tail: bool,
        value: Option<HirExpr>,
    },
    If {
        condition: HirExpr,
        then_block: HirBlock,
        else_branch: Option<Box<HirStmt>>,
    },
    While {
        condition: HirExpr,
        body: HirBlock,
    },
    ForC {
        init: Option<Box<HirStmt>>,
        condition: Option<HirExpr>,
        step: Option<Box<HirStmt>>,
        body: HirBlock,
    },
    ForEach {
        mutable: bool,
        pattern: HirPattern,
        iterable: HirExpr,
        body: HirBlock,
    },
    Break,
    Continue,
    DeferExpr {
        expr: HirExpr,
    },
    DeferBlock {
        block: HirBlock,
    },
    Unsafe {
        block: HirBlock,
    },
    WithContext {
        overrides: Vec<(String, HirExpr)>,
        body: HirBlock,
    },
    Select {
        arms: Vec<HirSelectArm>,
    },
    Block {
        block: HirBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HirSelectArm {
    Receive {
        channel: HirExpr,
        pattern: HirPattern,
        body: HirBlock,
    },
    Timeout {
        duration: HirExpr,
        body: HirBlock,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HirValueRef {
    pub root: ResolvedName,
    pub tail: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum HirTypeRef {
    Def(DefId),
    Import { import: u32, tail: Vec<String> },
    Builtin,
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum HirExprKind {
    Integer {
        text: String,
    },
    Float {
        text: String,
    },
    Character {
        value: char,
    },
    String {
        value: String,
    },
    CString {
        value: String,
    },
    Bool {
        value: bool,
    },
    None,
    Keyword {
        name: String,
    },
    Name {
        reference: HirValueRef,
    },
    Qualified {
        namespace: HirTypeRef,
        name: String,
    },
    Array {
        items: Vec<HirExpr>,
    },
    StructInit {
        namespace: HirTypeRef,
        variant: Option<String>,
        fields: Vec<(String, HirExpr)>,
    },
    Unary {
        op: ast::UnaryOp,
        value: Box<HirExpr>,
    },
    Binary {
        op: ast::BinaryOp,
        left: Box<HirExpr>,
        right: Box<HirExpr>,
    },
    Call {
        callee: Box<HirExpr>,
        args: Vec<HirCallArg>,
    },
    Index {
        base: Box<HirExpr>,
        index: Box<HirExpr>,
    },
    Member {
        base: Box<HirExpr>,
        name: String,
    },
    Try {
        value: Box<HirExpr>,
    },
    Closure {
        captures: Vec<HirCapture>,
        params: Vec<(LocalId, HirType)>,
        return_type: Option<HirType>,
        body: HirBlock,
    },
    Match {
        value: Box<HirExpr>,
        arms: Vec<HirMatchArm>,
    },
    ReaderForm {
        tag: String,
        value: ast::FdnValue,
    },
    Annotated {
        value: Box<HirExpr>,
        metadata: Vec<ast::Metadata>,
    },
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HirCallArg {
    Positional { value: HirExpr },
    Named { name: String, value: HirExpr },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirCapture {
    pub local: LocalId,
    pub source: ResolvedName,
    pub by_reference: bool,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirMatchArm {
    pub pattern: HirPattern,
    pub guard: Option<HirExpr>,
    pub body: HirMatchBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HirMatchBody {
    Block(HirBlock),
    Expr(HirExpr),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "pattern_kind", rename_all = "snake_case")]
pub enum HirPatternKind {
    Wildcard,
    Binding {
        local: LocalId,
        optional: bool,
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
        namespace: HirTypeRef,
        name: String,
        fields: Vec<HirPatternField>,
        explicit_braces: bool,
    },
    None {
        explicit_braces: bool,
    },
    Some {
        value: Box<HirPattern>,
    },
    Struct {
        path: HirTypeRef,
        fields: Vec<HirPatternField>,
    },
    Sequence {
        items: Vec<HirPattern>,
        rest: Option<LocalId>,
    },
    Map {
        entries: Vec<HirMapPatternEntry>,
        ignore_rest: bool,
    },
    Or {
        patterns: Vec<HirPattern>,
    },
    As {
        local: LocalId,
        pattern: Box<HirPattern>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirPatternField {
    pub name: String,
    pub pattern: Option<HirPattern>,
    pub shorthand_local: Option<LocalId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirMapPatternEntry {
    pub keyword: String,
    pub local: LocalId,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HirTypeKind {
    Named {
        reference: HirTypeRef,
    },
    Pointer {
        volatile: bool,
        inner: Box<HirType>,
    },
    Reference {
        mutable: bool,
        inner: Box<HirType>,
    },
    Optional {
        inner: Box<HirType>,
    },
    Slice {
        mutable: bool,
        element: Box<HirType>,
    },
    Array {
        element: Box<HirType>,
        length: Box<HirExpr>,
    },
    Result {
        ok: Box<HirType>,
        error: Box<HirType>,
    },
    Function {
        params: Vec<HirType>,
        result: Box<HirType>,
    },
    Closure {
        params: Vec<HirType>,
        result: Box<HirType>,
    },
    Annotated {
        inner: Box<HirType>,
        metadata: Vec<ast::Metadata>,
    },
}

pub fn lower_resolved_bodies(source: &ast::SourceFile, module: &HirModule) -> BodyHirOutput {
    let imports = source
        .imports
        .iter()
        .enumerate()
        .filter_map(|(i, path)| path.segments.last().map(|name| (name.clone(), i as u32)))
        .collect::<BTreeMap<_, _>>();
    let mut output = BodyHirOutput::default();

    for (index, declaration) in source.declarations.iter().enumerate() {
        let owner = DefId(index as u32);
        match &declaration.kind.kind {
            ast::DeclKind::Function(function) => {
                let mut lowerer = Lowerer::new(module, &imports, &mut output.diagnostics);
                lowerer.push_scope();
                let mut params = Vec::new();
                for parameter in &function.params {
                    let ty = lowerer.lower_type(&parameter.ty);
                    let id = lowerer.define_local(parameter.ty.span, false, true, &parameter.name);
                    params.push((id, ty));
                }
                let return_type = function
                    .return_type
                    .as_ref()
                    .map(|ty| lowerer.lower_type(ty));
                let block = lowerer.lower_block(&function.body, false);
                let locals = lowerer.locals;
                output.functions.insert(
                    owner,
                    HirBody {
                        owner,
                        params,
                        return_type,
                        locals,
                        block,
                    },
                );
            }
            ast::DeclKind::Global(value) => {
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
        }
    }
    output
}

struct Lowerer<'a, 'd> {
    module: &'a HirModule,
    imports: &'a BTreeMap<String, u32>,
    diagnostics: &'d mut Vec<HirDiagnostic>,
    scopes: Vec<BTreeMap<String, LocalId>>,
    locals: Vec<HirLocalDecl>,
    next_local: u32,
}

impl<'a, 'd> Lowerer<'a, 'd> {
    fn new(
        module: &'a HirModule,
        imports: &'a BTreeMap<String, u32>,
        diagnostics: &'d mut Vec<HirDiagnostic>,
    ) -> Self {
        Self {
            module,
            imports,
            diagnostics,
            scopes: Vec::new(),
            locals: Vec::new(),
            next_local: 0,
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(BTreeMap::new());
    }
    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_local(&mut self, span: Span, mutable: bool, parameter: bool, name: &str) -> LocalId {
        if self.scopes.is_empty() {
            self.push_scope();
        }
        let scope = self.scopes.last_mut().expect("scope");
        if scope.contains_key(name) {
            self.diagnostics.push(HirDiagnostic {
                span,
                message: format!("duplicate local definition `{name}` in the same lexical scope"),
            });
        }
        let id = LocalId(self.next_local);
        self.next_local += 1;
        scope.insert(name.to_owned(), id);
        self.locals.push(HirLocalDecl {
            id,
            span,
            mutable,
            parameter,
        });
        id
    }

    fn resolve_value(&mut self, name: &str, span: Span) -> ResolvedName {
        if let Some(id) = self.scopes.iter().rev().find_map(|s| s.get(name).copied()) {
            return ResolvedName::Local(id);
        }
        if let Some(id) = self.module.symbols.get(name).and_then(|s| s.value_def) {
            return ResolvedName::Def(id);
        }
        if let Some(index) = self.imports.get(name).copied() {
            return ResolvedName::Import(index);
        }
        if matches!(name, "Some") {
            return ResolvedName::BuiltinValue;
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!("unresolved value name `{name}`"),
        });
        ResolvedName::Error
    }

    fn lower_value_path(&mut self, path: &ast::Path, span: Span) -> HirValueRef {
        let first = path.segments.first().cloned().unwrap_or_default();
        let root = self.resolve_value(&first, span);
        HirValueRef {
            root,
            tail: path.segments.iter().skip(1).cloned().collect(),
        }
    }

    fn lower_type_ref(&mut self, path: &ast::Path, span: Span) -> HirTypeRef {
        if path.segments.len() == 1 {
            let name = &path.segments[0];
            if is_builtin_type(name) {
                return HirTypeRef::Builtin;
            }
            if let Some(id) = self.module.symbols.get(name).and_then(|s| s.type_def) {
                return HirTypeRef::Def(id);
            }
        }
        if let Some(first) = path.segments.first() {
            if let Some(import) = self.imports.get(first).copied() {
                return HirTypeRef::Import {
                    import,
                    tail: path.segments.iter().skip(1).cloned().collect(),
                };
            }
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!("unresolved type name `{}`", path.segments.join(".")),
        });
        HirTypeRef::Error
    }

    fn lower_type(&mut self, ty: &ast::TypeNode) -> HirType {
        let kind = match &ty.kind {
            TypeKind::Named { path } => HirTypeKind::Named {
                reference: self.lower_type_ref(path, ty.span),
            },
            TypeKind::Pointer { volatile, inner } => HirTypeKind::Pointer {
                volatile: *volatile,
                inner: Box::new(self.lower_type(inner)),
            },
            TypeKind::Reference { mutable, inner } => HirTypeKind::Reference {
                mutable: *mutable,
                inner: Box::new(self.lower_type(inner)),
            },
            TypeKind::Optional { inner } => HirTypeKind::Optional {
                inner: Box::new(self.lower_type(inner)),
            },
            TypeKind::Slice { mutable, element } => HirTypeKind::Slice {
                mutable: *mutable,
                element: Box::new(self.lower_type(element)),
            },
            TypeKind::Array { element, length } => HirTypeKind::Array {
                element: Box::new(self.lower_type(element)),
                length: Box::new(self.lower_expr(length)),
            },
            TypeKind::Result { ok, error } => HirTypeKind::Result {
                ok: Box::new(self.lower_type(ok)),
                error: Box::new(self.lower_type(error)),
            },
            TypeKind::Function { params, result } => HirTypeKind::Function {
                params: params.iter().map(|p| self.lower_type(p)).collect(),
                result: Box::new(self.lower_type(result)),
            },
            TypeKind::Closure { params, result } => HirTypeKind::Closure {
                params: params.iter().map(|p| self.lower_type(p)).collect(),
                result: Box::new(self.lower_type(result)),
            },
            TypeKind::Annotated { inner, metadata } => HirTypeKind::Annotated {
                inner: Box::new(self.lower_type(inner)),
                metadata: metadata.clone(),
            },
        };
        HirNode::new(ty.span, kind)
    }

    fn lower_block(&mut self, block: &ast::Block, scoped: bool) -> HirBlock {
        if scoped {
            self.push_scope();
        }
        let statements = block
            .statements
            .iter()
            .map(|s| self.lower_stmt(s))
            .collect();
        if scoped {
            self.pop_scope();
        }
        HirBlock {
            span: block.span,
            statements,
        }
    }

    fn lower_stmt(&mut self, stmt: &ast::Stmt) -> HirStmt {
        let kind = match &stmt.kind {
            StmtKind::Value(value) => {
                let ty = value.ty.as_ref().map(|t| self.lower_type(t));
                let expr = self.lower_expr(&value.value);
                let mutable = matches!(value.binding, ast::BindingKind::Var);
                let pattern = self.lower_binding_pattern(&value.pattern, mutable);
                HirStmtKind::Value {
                    mutable,
                    pattern,
                    ty,
                    value: expr,
                }
            }
            StmtKind::Assignment { target, value } => HirStmtKind::Assignment {
                target: self.lower_expr(target),
                value: self.lower_expr(value),
            },
            StmtKind::Expr { expr } => HirStmtKind::Expr {
                expr: self.lower_expr(expr),
            },
            StmtKind::Return { tail, value } => HirStmtKind::Return {
                tail: *tail,
                value: value.as_ref().map(|e| self.lower_expr(e)),
            },
            StmtKind::If {
                condition,
                then_block,
                else_branch,
            } => HirStmtKind::If {
                condition: self.lower_expr(condition),
                then_block: self.lower_block(then_block, true),
                else_branch: else_branch.as_ref().map(|s| Box::new(self.lower_stmt(s))),
            },
            StmtKind::While { condition, body } => HirStmtKind::While {
                condition: self.lower_expr(condition),
                body: self.lower_block(body, true),
            },
            StmtKind::ForEach {
                binding,
                pattern,
                iterable,
                body,
            } => {
                let iterable = self.lower_expr(iterable);
                self.push_scope();
                let mutable = matches!(binding, ast::BindingKind::Var);
                let pattern = self.lower_binding_pattern(pattern, mutable);
                let body = self.lower_block(body, false);
                self.pop_scope();
                HirStmtKind::ForEach {
                    mutable,
                    pattern,
                    iterable,
                    body,
                }
            }
            StmtKind::ForC {
                init,
                condition,
                step,
                body,
            } => {
                self.push_scope();
                let init = init
                    .as_ref()
                    .map(|i| Box::new(self.lower_for_init(i, stmt.span)));
                let condition = condition.as_ref().map(|e| self.lower_expr(e));
                let step = step
                    .as_ref()
                    .map(|s| Box::new(self.lower_for_step(s, stmt.span)));
                let body = self.lower_block(body, true);
                self.pop_scope();
                HirStmtKind::ForC {
                    init,
                    condition,
                    step,
                    body,
                }
            }
            StmtKind::Break => HirStmtKind::Break,
            StmtKind::Continue => HirStmtKind::Continue,
            StmtKind::Defer { body } => match body {
                ast::DeferBody::Expr(e) => HirStmtKind::DeferExpr {
                    expr: self.lower_expr(e),
                },
                ast::DeferBody::Block(b) => HirStmtKind::DeferBlock {
                    block: self.lower_block(b, true),
                },
            },
            StmtKind::Unsafe { body } => HirStmtKind::Unsafe {
                block: self.lower_block(body, true),
            },
            StmtKind::WithContext { overrides, body } => HirStmtKind::WithContext {
                overrides: overrides
                    .iter()
                    .map(|o| (o.name.clone(), self.lower_expr(&o.value)))
                    .collect(),
                body: self.lower_block(body, true),
            },
            StmtKind::Select { arms } => HirStmtKind::Select {
                arms: arms.iter().map(|a| self.lower_select_arm(a)).collect(),
            },
            StmtKind::Block { block } => HirStmtKind::Block {
                block: self.lower_block(block, true),
            },
        };
        HirNode::new(stmt.span, kind)
    }

    fn lower_for_init(&mut self, init: &ast::ForInit, span: Span) -> HirStmt {
        match init {
            ast::ForInit::Value(value) => {
                let ty = value.ty.as_ref().map(|t| self.lower_type(t));
                let expr = self.lower_expr(&value.value);
                let mutable = matches!(value.binding, ast::BindingKind::Var);
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
            }
            ast::ForInit::Assignment { target, value } => HirNode::new(
                span,
                HirStmtKind::Assignment {
                    target: self.lower_expr(target),
                    value: self.lower_expr(value),
                },
            ),
            ast::ForInit::Expr(expr) => HirNode::new(
                span,
                HirStmtKind::Expr {
                    expr: self.lower_expr(expr),
                },
            ),
        }
    }

    fn lower_for_step(&mut self, step: &ast::ForStep, span: Span) -> HirStmt {
        match step {
            ast::ForStep::Assignment { target, value } => HirNode::new(
                span,
                HirStmtKind::Assignment {
                    target: self.lower_expr(target),
                    value: self.lower_expr(value),
                },
            ),
            ast::ForStep::Expr(expr) => HirNode::new(
                span,
                HirStmtKind::Expr {
                    expr: self.lower_expr(expr),
                },
            ),
        }
    }

    fn lower_select_arm(&mut self, arm: &ast::SelectArm) -> HirSelectArm {
        match arm {
            ast::SelectArm::Receive {
                channel,
                pattern,
                body,
            } => {
                let channel = self.lower_expr(channel);
                self.push_scope();
                let pattern = self.lower_binding_pattern(pattern, false);
                let body = self.lower_block(body, false);
                self.pop_scope();
                HirSelectArm::Receive {
                    channel,
                    pattern,
                    body,
                }
            }
            ast::SelectArm::Timeout { duration, body } => HirSelectArm::Timeout {
                duration: self.lower_expr(duration),
                body: self.lower_block(body, true),
            },
        }
    }

    fn lower_expr(&mut self, expr: &ast::Expr) -> HirExpr {
        let kind = match &expr.kind {
            ExprKind::Integer { text } => HirExprKind::Integer { text: text.clone() },
            ExprKind::Float { text } => HirExprKind::Float { text: text.clone() },
            ExprKind::Character { value } => HirExprKind::Character { value: *value },
            ExprKind::String { value } => HirExprKind::String {
                value: value.clone(),
            },
            ExprKind::CString { value } => HirExprKind::CString {
                value: value.clone(),
            },
            ExprKind::Bool { value } => HirExprKind::Bool { value: *value },
            ExprKind::None => HirExprKind::None,
            ExprKind::Keyword { name } => HirExprKind::Keyword { name: name.clone() },
            ExprKind::Path { path } => HirExprKind::Name {
                reference: self.lower_value_path(path, expr.span),
            },
            ExprKind::Qualified { namespace, name } => HirExprKind::Qualified {
                namespace: self.lower_type_ref(namespace, expr.span),
                name: name.clone(),
            },
            ExprKind::Array { items } => HirExprKind::Array {
                items: items.iter().map(|e| self.lower_expr(e)).collect(),
            },
            ExprKind::StructInit {
                namespace,
                variant,
                fields,
                ..
            } => HirExprKind::StructInit {
                namespace: self.lower_type_ref(namespace, expr.span),
                variant: variant.clone(),
                fields: fields
                    .iter()
                    .map(|f| (f.name.clone(), self.lower_expr(&f.value)))
                    .collect(),
            },
            ExprKind::Unary { op, value } => HirExprKind::Unary {
                op: *op,
                value: Box::new(self.lower_expr(value)),
            },
            ExprKind::Binary { op, left, right } => HirExprKind::Binary {
                op: *op,
                left: Box::new(self.lower_expr(left)),
                right: Box::new(self.lower_expr(right)),
            },
            ExprKind::Call { callee, args } => HirExprKind::Call {
                callee: Box::new(self.lower_expr(callee)),
                args: args
                    .iter()
                    .map(|a| match a {
                        ast::CallArg::Positional { value } => HirCallArg::Positional {
                            value: self.lower_expr(value),
                        },
                        ast::CallArg::Named { name, value } => HirCallArg::Named {
                            name: name.clone(),
                            value: self.lower_expr(value),
                        },
                    })
                    .collect(),
            },
            ExprKind::Index { base, index } => HirExprKind::Index {
                base: Box::new(self.lower_expr(base)),
                index: Box::new(self.lower_expr(index)),
            },
            ExprKind::Member { base, name } => HirExprKind::Member {
                base: Box::new(self.lower_expr(base)),
                name: name.clone(),
            },
            ExprKind::Try { value } => HirExprKind::Try {
                value: Box::new(self.lower_expr(value)),
            },
            ExprKind::Closure {
                captures,
                params,
                return_type,
                body,
            } => {
                let mut resolved_captures = Vec::new();
                for capture in captures {
                    let source = self.resolve_value(&capture.name, expr.span);
                    resolved_captures.push((capture, source));
                }
                self.push_scope();
                let captures = resolved_captures
                    .into_iter()
                    .map(|(capture, source)| {
                        let local =
                            self.define_local(expr.span, capture.mutable, false, &capture.name);
                        HirCapture {
                            local,
                            source,
                            by_reference: capture.by_reference,
                            mutable: capture.mutable,
                        }
                    })
                    .collect();
                let params = params
                    .iter()
                    .map(|p| {
                        let ty = self.lower_type(&p.ty);
                        let local = self.define_local(p.ty.span, false, true, &p.name);
                        (local, ty)
                    })
                    .collect();
                let return_type = return_type.as_ref().map(|t| self.lower_type(t));
                let body = self.lower_block(body, false);
                self.pop_scope();
                HirExprKind::Closure {
                    captures,
                    params,
                    return_type,
                    body,
                }
            }
            ExprKind::Match { value, arms } => {
                let value = Box::new(self.lower_expr(value));
                let arms = arms
                    .iter()
                    .map(|arm| {
                        self.push_scope();
                        let pattern = self.lower_binding_pattern(&arm.pattern, false);
                        let guard = arm.guard.as_ref().map(|g| self.lower_expr(g));
                        let body = match &arm.body {
                            ast::MatchBody::Block(b) => {
                                HirMatchBody::Block(self.lower_block(b, false))
                            }
                            ast::MatchBody::Expr(e) => HirMatchBody::Expr(self.lower_expr(e)),
                        };
                        self.pop_scope();
                        HirMatchArm {
                            pattern,
                            guard,
                            body,
                        }
                    })
                    .collect();
                HirExprKind::Match { value, arms }
            }
            ExprKind::ReaderForm { tag, value } => HirExprKind::ReaderForm {
                tag: tag.clone(),
                value: value.clone(),
            },
            ExprKind::Annotated { value, metadata } => HirExprKind::Annotated {
                value: Box::new(self.lower_expr(value)),
                metadata: metadata.clone(),
            },
        };
        HirNode::new(expr.span, kind)
    }

    fn lower_binding_pattern(&mut self, pattern: &ast::Pattern, mutable: bool) -> HirPattern {
        let kind = match &pattern.kind {
            PatternKind::Wildcard => HirPatternKind::Wildcard,
            PatternKind::Binding { name, optional } => HirPatternKind::Binding {
                local: self.define_local(pattern.span, mutable, false, name),
                optional: *optional,
            },
            PatternKind::Literal { value } => HirPatternKind::Literal {
                value: value.clone(),
            },
            PatternKind::Range {
                start,
                end,
                inclusive,
            } => HirPatternKind::Range {
                start: start.clone(),
                end: end.clone(),
                inclusive: *inclusive,
            },
            PatternKind::Variant {
                namespace,
                name,
                fields,
                explicit_braces,
            } => HirPatternKind::Variant {
                namespace: self.lower_type_ref(namespace, pattern.span),
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|f| self.lower_pattern_field(f, mutable, pattern.span))
                    .collect(),
                explicit_braces: *explicit_braces,
            },
            PatternKind::None { explicit_braces } => HirPatternKind::None {
                explicit_braces: *explicit_braces,
            },
            PatternKind::Some { value } => HirPatternKind::Some {
                value: Box::new(self.lower_binding_pattern(value, mutable)),
            },
            PatternKind::Struct { path, fields } => HirPatternKind::Struct {
                path: self.lower_type_ref(path, pattern.span),
                fields: fields
                    .iter()
                    .map(|f| self.lower_pattern_field(f, mutable, pattern.span))
                    .collect(),
            },
            PatternKind::Sequence { items, rest } => HirPatternKind::Sequence {
                items: items
                    .iter()
                    .map(|p| self.lower_binding_pattern(p, mutable))
                    .collect(),
                rest: rest
                    .as_ref()
                    .map(|name| self.define_local(pattern.span, mutable, false, name)),
            },
            PatternKind::Map {
                entries,
                ignore_rest,
            } => HirPatternKind::Map {
                entries: entries
                    .iter()
                    .map(|e| HirMapPatternEntry {
                        keyword: e.keyword.clone(),
                        local: self.define_local(pattern.span, mutable, false, &e.binding),
                        optional: e.optional,
                    })
                    .collect(),
                ignore_rest: *ignore_rest,
            },
            PatternKind::Or { patterns } => HirPatternKind::Or {
                patterns: patterns
                    .iter()
                    .map(|p| self.lower_binding_pattern(p, mutable))
                    .collect(),
            },
            PatternKind::As {
                name,
                pattern: inner,
            } => HirPatternKind::As {
                local: self.define_local(pattern.span, mutable, false, name),
                pattern: Box::new(self.lower_binding_pattern(inner, mutable)),
            },
        };
        HirNode::new(pattern.span, kind)
    }

    fn lower_pattern_field(
        &mut self,
        field: &ast::PatternField,
        mutable: bool,
        span: Span,
    ) -> HirPatternField {
        if let Some(pattern) = &field.pattern {
            HirPatternField {
                name: field.name.clone(),
                pattern: Some(self.lower_binding_pattern(pattern, mutable)),
                shorthand_local: None,
            }
        } else {
            HirPatternField {
                name: field.name.clone(),
                pattern: None,
                shorthand_local: Some(self.define_local(span, mutable, false, &field.name)),
            }
        }
    }
}

fn is_builtin_type(name: &str) -> bool {
    matches!(
        name,
        "bool"
            | "char"
            | "str"
            | "void"
            | "never"
            | "usize"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "f32"
            | "f64"
    )
}
