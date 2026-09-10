use logos::Logos;

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip r"//[^\n]*")]
#[logos(skip r"/\*([^*]|\*[^/])*\*/")]
pub enum Token {
    /// Synthetic token inserted after a Logos lexing error so parsing can recover.
    Error,

    #[token("module")] Module,
    #[token("import")] Import,
    #[token("pub")] Pub,
    #[token("fn")] Fn,
    #[token("nfn")] Nfn,
    #[token("struct")] Struct,
    #[token("enum")] Enum,
    #[token("tagged")] Tagged,
    #[token("distinct")] Distinct,
    #[token("type")] Type,
    #[token("val")] Val,
    #[token("var")] Var,
    #[token("const")] Const,
    #[token("return")] Return,
    #[token("tail")] Tail,
    #[token("if")] If,
    #[token("else")] Else,
    #[token("while")] While,
    #[token("defer")] Defer,
    #[token("unsafe")] Unsafe,
    #[token("mut")] Mut,
    #[token("true")] True,
    #[token("false")] False,
    #[token("xor")] Xor,
    #[token("Result")] ResultType,
    #[token("closure")] ClosureType,

    #[regex(r#""([^"\\]|\\.)*""#, |lex| unquote(lex.slice()))]
    String(String),
    #[regex(
        r"([0-9][0-9_]*)\.[0-9][0-9_]*([eE][+-]?[0-9][0-9_]*)?(f32|f64)?",
        |lex| lex.slice().to_owned()
    )]
    Float(String),
    #[regex(
        r"(0[xX][0-9a-fA-F_]+|0[bB][01_]+|0[oO][0-7_]+|[0-9][0-9_]*)(i8|i16|i32|i64|isize|u8|u16|u32|u64|usize)?",
        |lex| lex.slice().to_owned()
    )]
    Integer(String),
    #[regex(r"[A-Za-z_][A-Za-z0-9_]*", |lex| lex.slice().to_owned())]
    Ident(String),

    #[token("->")] Arrow,
    #[token("=>")] FatArrow,
    #[token("::")] ColonColon,
    #[token("==")] EqEq,
    #[token("!=")] NotEq,
    #[token("<=")] LessEq,
    #[token(">=")] GreaterEq,
    #[token("&&")] AndAnd,
    #[token("||")] OrOr,
    #[token("<<")] ShiftLeft,
    #[token(">>")] ShiftRight,
    #[token("??")] Coalesce,

    #[token("(")] LParen,
    #[token(")")] RParen,
    #[token("{")] LBrace,
    #[token("}")] RBrace,
    #[token("[")] LBracket,
    #[token("]")] RBracket,
    #[token(",")] Comma,
    #[token(";")] Semicolon,
    #[token(":")] Colon,
    #[token(".")] Dot,
    #[token("?")] Question,
    #[token("=")] Eq,
    #[token("+")] Plus,
    #[token("-")] Minus,
    #[token("*")] Star,
    #[token("/")] Slash,
    #[token("%")] Percent,
    #[token("<")] Less,
    #[token(">")] Greater,
    #[token("&")] Amp,
    #[token("|")] Pipe,
    #[token("^")] Caret,
    #[token("!")] Bang,
    #[token("~")] Tilde,
}

fn unquote(text: &str) -> String {
    let inner = &text[1..text.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some('t') => out.push('\t'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Token::Ident(x) | Token::Integer(x) | Token::Float(x) => write!(f, "{x}"),
            Token::String(x) => write!(f, "\"{x}\""),
            Token::Error => write!(f, "<lexer-error>"),
            other => write!(f, "{:?}", other),
        }
    }
}
