use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Node<T> {
    pub span: Span,
    pub kind: T,
}

impl<T> Node<T> {
    pub const fn new(kind: T, span: Span) -> Self {
        Self { span, kind }
    }
}

pub type Expr = Node<ExprKind>;
pub type Stmt = Node<StmtKind>;
pub type Decl = Node<DeclKind>;
pub type TypeNode = Node<TypeKind>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Path {
    pub segments: Vec<String>,
}

impl Path {
    pub fn new(segments: Vec<String>) -> Self {
        Self { segments }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceFile {
    pub module: Path,
    pub imports: Vec<Path>,
    pub declarations: Vec<Decl>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decl", rename_all = "snake_case")]
pub enum DeclKind {
    Function(FunctionDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Tagged(TaggedDecl),
    Distinct(DistinctDecl),
    TypeAlias(TypeAliasDecl),
    Global { public: bool, value: ValueDecl },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionDecl {
    pub public: bool,
    pub named_arguments: bool,
    pub name: String,
    pub params: Vec<Param>,
    pub return_type: Option<TypeNode>,
    pub body: Block,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Param {
    pub name: String,
    pub ty: TypeNode,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StructDecl {
    pub public: bool,
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldDecl {
    pub name: String,
    pub ty: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumDecl {
    pub public: bool,
    pub name: String,
    pub variants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaggedDecl {
    pub public: bool,
    pub name: String,
    pub variants: Vec<TaggedVariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaggedVariant {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DistinctDecl {
    pub public: bool,
    pub name: String,
    pub underlying: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeAliasDecl {
    pub public: bool,
    pub name: String,
    pub target: TypeNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingKind {
    Val,
    Var,
    Const,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValueDecl {
    pub binding: BindingKind,
    pub name: String,
    pub ty: Option<TypeNode>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Block {
    pub span: Span,
    pub statements: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "stmt", rename_all = "snake_case")]
pub enum StmtKind {
    Value(ValueDecl),
    Expr {
        expr: Expr,
    },
    Return {
        tail: bool,
        value: Option<Expr>,
    },
    If {
        condition: Expr,
        then_block: Block,
        else_block: Option<Block>,
    },
    While {
        condition: Expr,
        body: Block,
    },
    Defer {
        body: Block,
    },
    Unsafe {
        body: Block,
    },
    Block {
        block: Block,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum ExprKind {
    Integer {
        text: String,
    },
    Float {
        text: String,
    },
    String {
        value: String,
    },
    Bool {
        value: bool,
    },
    Path {
        path: Path,
    },
    Array {
        items: Vec<Expr>,
    },
    StructInit {
        ty: Path,
        fields: Vec<InitField>,
    },
    Unary {
        op: UnaryOp,
        value: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InitField {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOp {
    Neg,
    Not,
    BitNot,
    AddressOf,
    Deref,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    ShiftLeft,
    ShiftRight,
    Less,
    LessEq,
    Greater,
    GreaterEq,
    Eq,
    NotEq,
    BitAnd,
    BitXor,
    BitOr,
    LogicalAnd,
    LogicalXor,
    LogicalOr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TypeKind {
    Named {
        path: Path,
    },
    Pointer {
        inner: Box<TypeNode>,
    },
    Reference {
        mutable: bool,
        inner: Box<TypeNode>,
    },
    Optional {
        inner: Box<TypeNode>,
    },
    Slice {
        mutable: bool,
        element: Box<TypeNode>,
    },
    Array {
        element: Box<TypeNode>,
        length: Box<Expr>,
    },
    Result {
        ok: Box<TypeNode>,
        error: Box<TypeNode>,
    },
    Function {
        params: Vec<TypeNode>,
        result: Box<TypeNode>,
    },
    Closure {
        params: Vec<TypeNode>,
        result: Box<TypeNode>,
    },
}
