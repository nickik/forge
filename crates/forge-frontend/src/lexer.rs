use logos::{FilterResult, Logos};

#[derive(Logos, Debug, Clone, PartialEq)]
#[logos(skip r"[ \t\r\n\f]+")]
#[logos(skip(r"//[^\n]*", allow_greedy = true))]
pub enum Token {
    /// Synthetic token inserted after a Logos lexing error so parsing can recover.
    Error,

    // Nested comments require a callback; regular expressions cannot count nesting.
    #[token("/*", skip_nested_block_comment)]
    BlockComment,

    // Core declarations and visibility.
    #[token("module")]
    Module,
    #[token("import")]
    Import,
    #[token("pub")]
    Pub,
    #[token("internal")]
    InternalReserved,
    #[token("fn")]
    Fn,
    #[token("nfn")]
    Nfn,
    #[token("struct")]
    Struct,
    #[token("enum")]
    Enum,
    #[token("tagged")]
    Tagged,
    #[token("bitstruct")]
    BitStruct,
    #[token("distinct")]
    Distinct,
    #[token("type")]
    Type,
    #[token("impl")]
    Impl,
    #[token("extern")]
    Extern,

    // Bindings and flow control.
    #[token("val")]
    Val,
    #[token("var")]
    Var,
    #[token("const")]
    Const,
    #[token("return")]
    Return,
    #[token("tail")]
    Tail,
    #[token("if")]
    If,
    #[token("else")]
    Else,
    #[token("while")]
    While,
    #[token("for")]
    For,
    #[token("in")]
    In,
    #[token("break")]
    Break,
    #[token("continue")]
    Continue,
    #[token("match")]
    Match,
    #[token("when")]
    When,
    #[token("switch")]
    SwitchReserved,
    #[token("defer")]
    Defer,
    #[token("unsafe")]
    Unsafe,
    #[token("with")]
    With,
    #[token("context")]
    Context,
    #[token("select")]
    Select,
    #[token("recv")]
    Recv,
    #[token("timeout")]
    Timeout,

    // Type/control words.
    #[token("mut")]
    Mut,
    #[token("volatile")]
    Volatile,
    #[token("true")]
    True,
    #[token("false")]
    False,
    #[token("None")]
    None,
    #[token("xor")]
    Xor,
    #[token("Result")]
    ResultType,
    #[token("closure")]
    ClosureType,

    // Literals. Specific prefixed forms appear before identifiers.
    #[regex(r#"c\"([^\"\\]|\\.)*\""#, |lex| unquote_c_string(lex.slice()))]
    CString(String),
    #[regex(r#"\"([^\"\\]|\\.)*\""#, |lex| unquote_string(lex.slice()))]
    String(String),
    #[regex(r#"'([^'\\]|\\.)'"#, |lex| unquote_char(lex.slice()))]
    Character(char),
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

    // Multi-character punctuation/operators, longest forms first.
    #[token("..=")]
    DotDotEq,
    #[token("..")]
    DotDot,
    #[token("->")]
    Arrow,
    #[token("=>")]
    FatArrow,
    #[token("::")]
    ColonColon,
    #[token("==")]
    EqEq,
    #[token("!=")]
    NotEq,
    #[token("<=")]
    LessEq,
    #[token(">=")]
    GreaterEq,
    #[token("&&")]
    AndAnd,
    #[token("||")]
    OrOr,
    #[token("<<=")]
    ShiftLeftEqReserved,
    #[token(">>=")]
    ShiftRightEqReserved,
    #[token("<<")]
    ShiftLeft,
    #[token(">>")]
    ShiftRight,
    #[token("+=")]
    PlusEqReserved,
    #[token("-=")]
    MinusEqReserved,
    #[token("*=")]
    StarEqReserved,
    #[token("/=")]
    SlashEqReserved,
    #[token("%=")]
    PercentEqReserved,
    #[token("&=")]
    AmpEqReserved,
    #[token("|=")]
    PipeEqReserved,
    #[token("^=")]
    CaretEqReserved,

    // Single-character punctuation/operators.
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token(",")]
    Comma,
    #[token(";")]
    Semicolon,
    #[token(":")]
    Colon,
    #[token(".")]
    Dot,
    #[token("?")]
    Question,
    #[token("=")]
    Eq,
    #[token("+")]
    Plus,
    #[token("-")]
    Minus,
    #[token("*")]
    Star,
    #[token("/")]
    Slash,
    #[token("%")]
    Percent,
    #[token("<")]
    Less,
    #[token(">")]
    Greater,
    #[token("&")]
    Amp,
    #[token("|")]
    Pipe,
    #[token("^")]
    Caret,
    #[token("!")]
    Bang,
    #[token("~")]
    Tilde,
    #[token("@")]
    At,
    #[token("#")]
    Hash,
}

fn skip_nested_block_comment(lex: &mut logos::Lexer<'_, Token>) -> FilterResult<(), ()> {
    let bytes = lex.remainder().as_bytes();
    let mut depth = 1usize;
    let mut index = 0usize;

    while index + 1 < bytes.len() {
        match (bytes[index], bytes[index + 1]) {
            (b'/', b'*') => {
                depth += 1;
                index += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                index += 2;
                if depth == 0 {
                    lex.bump(index);
                    return FilterResult::Skip;
                }
            }
            _ => index += 1,
        }
    }

    // Consume the remainder so a single diagnostic covers the unterminated comment.
    lex.bump(bytes.len());
    FilterResult::Error(())
}

fn unquote_string(text: &str) -> String {
    unescape(&text[1..text.len() - 1])
}

fn unquote_c_string(text: &str) -> String {
    unescape(&text[2..text.len() - 1])
}

fn unquote_char(text: &str) -> char {
    let value = unescape(&text[1..text.len() - 1]);
    value.chars().next().expect("character token is non-empty")
}

fn unescape(inner: &str) -> String {
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
            Some('\'') => out.push('\''),
            Some('"') => out.push('"'),
            Some(other) => {
                // The parser retains unknown escapes literally for now; semantic/string
                // validation can tighten the accepted escape set independently.
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
            Token::CString(x) => write!(f, "c\"{x}\""),
            Token::Character(x) => write!(f, "'{x}'"),
            Token::Error => write!(f, "<lexer-error>"),
            other => write!(f, "{:?}", other),
        }
    }
}
