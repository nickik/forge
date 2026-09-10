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
pub type Decl = Node<DeclData>;
pub type TypeNode = Node<TypeKind>;
pub type Pattern = Node<PatternKind>;

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
pub struct DeclData {
    pub public: bool,
    pub metadata: Vec<Metadata>,
    pub kind: DeclKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "decl", rename_all = "snake_case")]
pub enum DeclKind {
    Function(FunctionDecl),
    Struct(StructDecl),
    Enum(EnumDecl),
    Tagged(TaggedDecl),
    BitStruct(BitStructDecl),
    Distinct(DistinctDecl),
    TypeAlias(TypeAliasDecl),
    Impl(ImplDecl),
    Global(ValueDecl),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FunctionDecl {
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
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FieldDecl {
    pub metadata: Vec<Metadata>,
    pub name: String,
    pub ty: TypeNode,
    pub default: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumDecl {
    pub name: String,
    pub variants: Vec<EnumVariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EnumVariant {
    pub name: String,
    pub value: Option<Expr>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaggedDecl {
    pub name: String,
    pub variants: Vec<TaggedVariant>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TaggedVariant {
    pub name: String,
    pub fields: Vec<FieldDecl>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BitStructDecl {
    pub name: String,
    pub storage: TypeNode,
    pub fields: Vec<BitField>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BitField {
    pub name: String,
    pub width: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DistinctDecl {
    pub name: String,
    pub underlying: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeAliasDecl {
    pub name: String,
    pub target: TypeNode,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImplDecl {
    pub target: Path,
    pub methods: Vec<ImplMethod>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ImplMethod {
    pub metadata: Vec<Metadata>,
    pub function: FunctionDecl,
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
    pub pattern: Pattern,
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
    Assignment { target: Expr, value: Expr },
    Expr { expr: Expr },
    Return { tail: bool, value: Option<Expr> },
    If {
        condition: Expr,
        then_block: Block,
        else_branch: Option<Box<Stmt>>,
    },
    While { condition: Expr, body: Block },
    ForC {
        init: Option<ForInit>,
        condition: Option<Expr>,
        step: Option<ForStep>,
        body: Block,
    },
    ForEach {
        binding: BindingKind,
        pattern: Pattern,
        iterable: Expr,
        body: Block,
    },
    Break,
    Continue,
    Defer { body: DeferBody },
    Unsafe { body: Block },
    WithContext {
        overrides: Vec<ContextOverride>,
        body: Block,
    },
    Select { arms: Vec<SelectArm> },
    Block { block: Block },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContextOverride {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForInit {
    Value(ValueDecl),
    Assignment { target: Expr, value: Expr },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForStep {
    Assignment { target: Expr, value: Expr },
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeferBody {
    Block(Block),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MatchArm {
    pub pattern: Pattern,
    pub guard: Option<Expr>,
    pub body: MatchBody,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MatchBody {
    Block(Block),
    Expr(Expr),
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SelectArm {
    Receive {
        channel: Expr,
        pattern: Pattern,
        body: Block,
    },
    Timeout { duration: Expr, body: Block },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "expr", rename_all = "snake_case")]
pub enum ExprKind {
    Integer { text: String },
    Float { text: String },
    Character { value: char },
    String { value: String },
    CString { value: String },
    Bool { value: bool },
    None,
    Keyword { name: String },
    Path { path: Path },
    Qualified { namespace: Path, name: String },
    Array { items: Vec<Expr> },
    StructInit {
        namespace: Path,
        variant: Option<String>,
        fields: Vec<InitField>,
        explicit_braces: bool,
    },
    Unary { op: UnaryOp, value: Box<Expr> },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Call { callee: Box<Expr>, args: Vec<CallArg> },
    Index { base: Box<Expr>, index: Box<Expr> },
    Member { base: Box<Expr>, name: String },
    Try { value: Box<Expr> },
    Closure {
        captures: Vec<Capture>,
        params: Vec<Param>,
        return_type: Option<TypeNode>,
        body: Block,
    },
    Match { value: Box<Expr>, arms: Vec<MatchArm> },
    ReaderForm { tag: String, value: FdnValue },
    Annotated { value: Box<Expr>, metadata: Vec<Metadata> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CallArg {
    Positional { value: Expr },
    Named { name: String, value: Expr },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InitField {
    pub name: String,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capture {
    pub name: String,
    pub by_reference: bool,
    pub mutable: bool,
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
    Named { path: Path },
    Pointer { volatile: bool, inner: Box<TypeNode> },
    Reference { mutable: bool, inner: Box<TypeNode> },
    Optional { inner: Box<TypeNode> },
    Slice { mutable: bool, element: Box<TypeNode> },
    Array { element: Box<TypeNode>, length: Box<Expr> },
    Result { ok: Box<TypeNode>, error: Box<TypeNode> },
    Function { params: Vec<TypeNode>, result: Box<TypeNode> },
    Closure { params: Vec<TypeNode>, result: Box<TypeNode> },
    Annotated { inner: Box<TypeNode>, metadata: Vec<Metadata> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Metadata {
    pub name: Option<String>,
    pub arguments: Vec<MetadataArg>,
    pub map: Option<FdnValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MetadataArg {
    Value(FdnValue),
    Named { name: String, value: FdnValue },
    Range {
        start: Box<FdnValue>,
        end: Box<FdnValue>,
        inclusive: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "pattern", rename_all = "snake_case")]
pub enum PatternKind {
    Wildcard,
    Binding { name: String, optional: bool },
    Literal { value: PatternLiteral },
    Range {
        start: PatternLiteral,
        end: PatternLiteral,
        inclusive: bool,
    },
    Variant {
        namespace: Path,
        name: String,
        fields: Vec<PatternField>,
        explicit_braces: bool,
    },
    None { explicit_braces: bool },
    Some { value: Box<Pattern> },
    Struct { path: Path, fields: Vec<PatternField> },
    Sequence { items: Vec<Pattern>, rest: Option<String> },
    Map { entries: Vec<MapPatternEntry>, ignore_rest: bool },
    Or { patterns: Vec<Pattern> },
    As { name: String, pattern: Box<Pattern> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PatternLiteral {
    Integer { text: String },
    Character { value: char },
    String { value: String },
    Bool { value: bool },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PatternField {
    pub name: String,
    pub pattern: Option<Pattern>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MapPatternEntry {
    pub keyword: String,
    pub binding: String,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "fdn", rename_all = "snake_case")]
pub enum FdnValue {
    Nil,
    Bool { value: bool },
    Integer { text: String },
    Float { text: String },
    String { value: String },
    Character { value: char },
    Keyword { name: String },
    Symbol { name: String },
    Vector { values: Vec<FdnValue> },
    List { values: Vec<FdnValue> },
    Map { entries: Vec<FdnMapEntry> },
    Set { values: Vec<FdnValue> },
    Tagged { tag: String, value: Box<FdnValue> },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FdnMapEntry {
    pub key: FdnValue,
    pub value: FdnValue,
}
