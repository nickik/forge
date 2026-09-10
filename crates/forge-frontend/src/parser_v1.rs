use chumsky::{
    input::{Stream, ValueInput},
    prelude::*,
};
use logos::Logos;

use crate::ast::*;
use crate::lexer::Token;

pub type CSpan = SimpleSpan<usize>;
type ParseExtra<'tokens> = extra::Err<Rich<'tokens, Token, CSpan>>;

#[derive(Debug, Clone, serde::Serialize)]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, serde::Serialize)]
pub struct ParseOutput {
    pub ast: Option<SourceFile>,
    pub diagnostics: Vec<Diagnostic>,
}

fn span(value: CSpan) -> Span {
    Span::new(value.start, value.end)
}

fn ident<'tokens, I>() -> impl Parser<'tokens, I, String, ParseExtra<'tokens>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    select! { Token::Ident(name) => name }
}

fn path<'tokens, I>() -> impl Parser<'tokens, I, Path, ParseExtra<'tokens>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    ident()
        .separated_by(just(Token::Dot))
        .at_least(1)
        .collect::<Vec<_>>()
        .map(Path::new)
        .boxed()
}

fn fdn_word<'tokens, I>() -> impl Parser<'tokens, I, String, ParseExtra<'tokens>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    select! {
        Token::Ident(name) => name,
        Token::Module => "module".to_owned(),
        Token::Import => "import".to_owned(),
        Token::Pub => "pub".to_owned(),
        Token::InternalReserved => "internal".to_owned(),
        Token::Fn => "fn".to_owned(),
        Token::Nfn => "nfn".to_owned(),
        Token::Struct => "struct".to_owned(),
        Token::Enum => "enum".to_owned(),
        Token::Tagged => "tagged".to_owned(),
        Token::BitStruct => "bitstruct".to_owned(),
        Token::Distinct => "distinct".to_owned(),
        Token::Type => "type".to_owned(),
        Token::Impl => "impl".to_owned(),
        Token::Extern => "extern".to_owned(),
        Token::Val => "val".to_owned(),
        Token::Var => "var".to_owned(),
        Token::Const => "const".to_owned(),
        Token::Return => "return".to_owned(),
        Token::Tail => "tail".to_owned(),
        Token::If => "if".to_owned(),
        Token::Else => "else".to_owned(),
        Token::While => "while".to_owned(),
        Token::For => "for".to_owned(),
        Token::In => "in".to_owned(),
        Token::Break => "break".to_owned(),
        Token::Continue => "continue".to_owned(),
        Token::Match => "match".to_owned(),
        Token::When => "when".to_owned(),
        Token::SwitchReserved => "switch".to_owned(),
        Token::Defer => "defer".to_owned(),
        Token::Unsafe => "unsafe".to_owned(),
        Token::With => "with".to_owned(),
        Token::Context => "context".to_owned(),
        Token::Select => "select".to_owned(),
        Token::Recv => "recv".to_owned(),
        Token::Timeout => "timeout".to_owned(),
        Token::Mut => "mut".to_owned(),
        Token::Volatile => "volatile".to_owned(),
        Token::None => "None".to_owned(),
        Token::Some => "Some".to_owned(),
        Token::Xor => "xor".to_owned(),
        Token::ResultType => "Result".to_owned(),
        Token::ClosureType => "closure".to_owned(),
    }
}

fn fdn_name<'tokens, I>() -> impl Parser<'tokens, I, String, ParseExtra<'tokens>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    let separator = choice((just(Token::Slash).to('/'), just(Token::Dot).to('.')));
    fdn_word()
        .then(separator.then(fdn_word()).repeated().collect::<Vec<_>>())
        .map(|(first, rest)| {
            let mut name = first;
            for (separator, part) in rest {
                name.push(separator);
                name.push_str(&part);
            }
            name
        })
        .boxed()
}

fn source_parser<'tokens, I>() -> impl Parser<'tokens, I, SourceFile, ParseExtra<'tokens>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    let mut fdn = Recursive::declare();
    let mut ty = Recursive::declare();
    let mut pattern = Recursive::declare();
    let mut expr = Recursive::declare();
    let mut stmt = Recursive::declare();

    // FDN embedded in metadata/reader forms.
    let fdn_scalar = choice((
        just(Token::True).to(FdnValue::Bool { value: true }),
        just(Token::False).to(FdnValue::Bool { value: false }),
        select! { Token::Integer(text) => FdnValue::Integer { text } },
        select! { Token::Float(text) => FdnValue::Float { text } },
        select! { Token::String(value) => FdnValue::String { value } },
        select! { Token::Character(value) => FdnValue::Character { value } },
        just(Token::Colon)
            .ignore_then(fdn_name())
            .map(|name| FdnValue::Keyword { name }),
        fdn_name().map(|name| {
            if name == "nil" {
                FdnValue::Nil
            } else {
                FdnValue::Symbol { name }
            }
        }),
    ))
    .boxed();

    let optional_comma = just(Token::Comma).or_not().ignored();
    let fdn_vector = fdn
        .clone()
        .then_ignore(optional_comma.clone())
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map(|values| FdnValue::Vector { values });
    let fdn_list = fdn
        .clone()
        .then_ignore(optional_comma.clone())
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen))
        .map(|values| FdnValue::List { values });
    let fdn_map = fdn
        .clone()
        .then(fdn.clone())
        .map(|(key, value)| FdnMapEntry { key, value })
        .then_ignore(optional_comma.clone())
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map(|entries| FdnValue::Map { entries });
    let fdn_set = just(Token::Hash)
        .ignore_then(
            fdn.clone()
                .then_ignore(optional_comma.clone())
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|values| FdnValue::Set { values });
    let fdn_tagged = just(Token::Hash)
        .ignore_then(fdn_name())
        .then(fdn.clone())
        .map(|(tag, value)| FdnValue::Tagged {
            tag,
            value: Box::new(value),
        });
    fdn.define(
        choice((fdn_set, fdn_tagged, fdn_vector, fdn_list, fdn_map, fdn_scalar))
            .labelled("FDN value")
            .boxed(),
    );

    // Metadata.
    let metadata_named_arg = ident()
        .then_ignore(just(Token::Colon))
        .then(fdn.clone())
        .map(|(name, value)| MetadataArg::Named { name, value });
    let metadata_range_arg = fdn
        .clone()
        .then(choice((
            just(Token::DotDotEq).to(true),
            just(Token::DotDot).to(false),
        )))
        .then(fdn.clone())
        .map(|((start, inclusive), end)| MetadataArg::Range {
            start: Box::new(start),
            end: Box::new(end),
            inclusive,
        });
    let metadata_arg = choice((
        metadata_named_arg,
        metadata_range_arg,
        fdn.clone().map(MetadataArg::Value),
    ))
    .boxed();
    let named_metadata = just(Token::At)
        .ignore_then(ident())
        .then(
            metadata_arg
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen))
                .or_not(),
        )
        .map(|(name, arguments)| Metadata {
            name: Some(name),
            arguments: arguments.unwrap_or_default(),
            map: None,
        });
    let structured_metadata = just(Token::At)
        .ignore_then(
            fdn.clone()
                .filter(|value| matches!(value, FdnValue::Map { .. })),
        )
        .map(|map| Metadata {
            name: None,
            arguments: Vec::new(),
            map: Some(map),
        });
    let metadata = choice((structured_metadata, named_metadata)).boxed();

    // Types.
    let named_type = path().map_with(|path, e| Node::new(TypeKind::Named { path }, span(e.span())));
    let result_type = just(Token::ResultType)
        .ignore_then(
            ty.clone()
                .then_ignore(just(Token::Comma))
                .then(ty.clone())
                .delimited_by(just(Token::LBracket), just(Token::RBracket)),
        )
        .map_with(|(ok, error), e| {
            Node::new(
                TypeKind::Result {
                    ok: Box::new(ok),
                    error: Box::new(error),
                },
                span(e.span()),
            )
        });
    let array_type = ty
        .clone()
        .then_ignore(just(Token::Semicolon))
        .then(expr.clone())
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map_with(|(element, length), e| {
            Node::new(
                TypeKind::Array {
                    element: Box::new(element),
                    length: Box::new(length),
                },
                span(e.span()),
            )
        });
    let callable_types = ty
        .clone()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    let function_type = just(Token::Fn)
        .ignore_then(callable_types.clone())
        .then_ignore(just(Token::Arrow))
        .then(ty.clone())
        .map_with(|(params, result), e| {
            Node::new(
                TypeKind::Function {
                    params,
                    result: Box::new(result),
                },
                span(e.span()),
            )
        });
    let closure_type = just(Token::ClosureType)
        .ignore_then(callable_types)
        .then_ignore(just(Token::Arrow))
        .then(ty.clone())
        .map_with(|(params, result), e| {
            Node::new(
                TypeKind::Closure {
                    params,
                    result: Box::new(result),
                },
                span(e.span()),
            )
        });

    #[derive(Clone)]
    enum TypePrefix {
        Pointer(bool),
        Reference(bool),
    }
    let type_prefix = choice((
        just(Token::Star)
            .ignore_then(just(Token::Volatile).or_not())
            .map(|value| TypePrefix::Pointer(value.is_some())),
        just(Token::Amp)
            .ignore_then(just(Token::Mut).or_not())
            .map(|value| TypePrefix::Reference(value.is_some())),
    ));
    let type_base = choice((result_type, array_type, function_type, closure_type, named_type)).boxed();
    let prefixed_type = type_prefix
        .repeated()
        .foldr_with(type_base, |prefix, inner, e| {
            let kind = match prefix {
                TypePrefix::Pointer(volatile) => TypeKind::Pointer {
                    volatile,
                    inner: Box::new(inner),
                },
                TypePrefix::Reference(mutable) => TypeKind::Reference {
                    mutable,
                    inner: Box::new(inner),
                },
            };
            Node::new(kind, span(e.span()))
        });
    #[derive(Clone)]
    enum TypeSuffix {
        Optional,
        Slice(bool),
        Metadata(Metadata),
    }
    let type_suffix = choice((
        just(Token::Question).to(TypeSuffix::Optional),
        just(Token::LBracket)
            .ignore_then(just(Token::RBracket))
            .ignore_then(just(Token::Mut).or_not())
            .map(|mutable| TypeSuffix::Slice(mutable.is_some())),
        metadata.clone().map(TypeSuffix::Metadata),
    ));
    ty.define(
        prefixed_type
            .foldl_with(type_suffix.repeated(), |base, suffix, e| {
                let kind = match suffix {
                    TypeSuffix::Optional => TypeKind::Optional {
                        inner: Box::new(base),
                    },
                    TypeSuffix::Slice(mutable) => TypeKind::Slice {
                        mutable,
                        element: Box::new(base),
                    },
                    TypeSuffix::Metadata(item) => TypeKind::Annotated {
                        inner: Box::new(base),
                        metadata: vec![item],
                    },
                };
                Node::new(kind, span(e.span()))
            })
            .labelled("type")
            .boxed(),
    );

    // Patterns.
    let signed_integer = just(Token::Minus)
        .ignore_then(select! { Token::Integer(text) => text })
        .map(|text| format!("-{text}"));
    let pattern_literal = choice((
        signed_integer.map(|text| PatternLiteral::Integer { text }),
        select! { Token::Integer(text) => PatternLiteral::Integer { text } },
        select! { Token::Character(value) => PatternLiteral::Character { value } },
        select! { Token::String(value) => PatternLiteral::String { value } },
        just(Token::True).to(PatternLiteral::Bool { value: true }),
        just(Token::False).to(PatternLiteral::Bool { value: false }),
    ))
    .boxed();
    let literal_pattern = pattern_literal
        .clone()
        .then(
            choice((
                just(Token::DotDotEq).to(true),
                just(Token::DotDot).to(false),
            ))
            .then(pattern_literal.clone())
            .or_not(),
        )
        .map_with(|(start, range), e| match range {
            Some((inclusive, end)) => Node::new(
                PatternKind::Range {
                    start,
                    end,
                    inclusive,
                },
                span(e.span()),
            ),
            None => Node::new(PatternKind::Literal { value: start }, span(e.span())),
        });
    let pattern_field = ident()
        .then(just(Token::Colon).ignore_then(pattern.clone()).or_not())
        .map(|(name, pattern)| PatternField { name, pattern });
    let pattern_fields = pattern_field
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace));
    let variant_pattern = path()
        .then_ignore(just(Token::ColonColon))
        .then(ident())
        .then(pattern_fields.clone().or_not())
        .map_with(|((namespace, name), fields), e| {
            Node::new(
                PatternKind::Variant {
                    namespace,
                    name,
                    explicit_braces: fields.is_some(),
                    fields: fields.unwrap_or_default(),
                },
                span(e.span()),
            )
        });
    let struct_pattern = path()
        .then(pattern_fields.clone())
        .map_with(|(path, fields), e| {
            Node::new(PatternKind::Struct { path, fields }, span(e.span()))
        });
    let none_pattern = just(Token::None)
        .then(
            just(Token::LBrace)
                .ignore_then(just(Token::RBrace))
                .or_not(),
        )
        .map_with(|(_, braces), e| {
            Node::new(
                PatternKind::None {
                    explicit_braces: braces.is_some(),
                },
                span(e.span()),
            )
        });
    let some_pattern = just(Token::Some)
        .ignore_then(pattern.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .map_with(|value, e| {
            Node::new(
                PatternKind::Some {
                    value: Box::new(value),
                },
                span(e.span()),
            )
        });
    let sequence_rest = just(Token::DotDot)
        .ignore_then(ident().or_not())
        .map(|name| name.unwrap_or_default());
    let sequence_pattern = pattern
        .clone()
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .then(just(Token::Comma).or_not().ignore_then(sequence_rest).or_not())
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map_with(|(items, rest), e| {
            Node::new(PatternKind::Sequence { items, rest }, span(e.span()))
        });
    let map_entry = just(Token::Colon)
        .ignore_then(fdn_name())
        .then(ident())
        .then(just(Token::Question).or_not())
        .map(|((keyword, binding), optional)| MapPatternEntry {
            keyword,
            binding,
            optional: optional.is_some(),
        });
    let map_pattern = map_entry
        .separated_by(just(Token::Comma))
        .collect::<Vec<_>>()
        .then(
            just(Token::Comma)
                .or_not()
                .ignore_then(just(Token::DotDot))
                .or_not(),
        )
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map_with(|(entries, rest), e| {
            Node::new(
                PatternKind::Map {
                    entries,
                    ignore_rest: rest.is_some(),
                },
                span(e.span()),
            )
        });
    let wildcard = just(Token::Underscore)
        .map_with(|_, e| Node::new(PatternKind::Wildcard, span(e.span())));
    let binding = ident().map_with(|name, e| {
        Node::new(
            PatternKind::Binding {
                name,
                optional: false,
            },
            span(e.span()),
        )
    });
    let base_pattern = choice((
        variant_pattern,
        none_pattern,
        some_pattern,
        struct_pattern,
        sequence_pattern,
        map_pattern,
        literal_pattern,
        wildcard,
        binding,
    ))
    .boxed();
    let as_pattern = ident()
        .then_ignore(just(Token::At))
        .then(base_pattern.clone())
        .map_with(|(name, pattern), e| {
            Node::new(
                PatternKind::As {
                    name,
                    pattern: Box::new(pattern),
                },
                span(e.span()),
            )
        })
        .or(base_pattern)
        .boxed();
    pattern.define(
        as_pattern
            .clone()
            .separated_by(just(Token::Pipe))
            .at_least(1)
            .collect::<Vec<_>>()
            .map_with(|patterns, e| {
                if patterns.len() == 1 {
                    patterns.into_iter().next().expect("one pattern")
                } else {
                    Node::new(PatternKind::Or { patterns }, span(e.span()))
                }
            })
            .labelled("pattern")
            .boxed(),
    );

    // Blocks can refer to the declared statement parser before it is defined.
    let block = stmt
        .clone()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map_with(|statements, e| Block {
            span: span(e.span()),
            statements,
        })
        .boxed();

    // Expressions.
    let literal_expr = select! {
        Token::Integer(text) => ExprKind::Integer { text },
        Token::Float(text) => ExprKind::Float { text },
        Token::Character(value) => ExprKind::Character { value },
        Token::String(value) => ExprKind::String { value },
        Token::CString(value) => ExprKind::CString { value },
        Token::True => ExprKind::Bool { value: true },
        Token::False => ExprKind::Bool { value: false },
    }
    .map_with(|kind, e| Node::new(kind, span(e.span())));
    let none_expr = just(Token::None)
        .then(
            just(Token::LBrace)
                .ignore_then(just(Token::RBrace))
                .or_not(),
        )
        .map_with(|_, e| Node::new(ExprKind::None, span(e.span())));
    let some_expr = just(Token::Some).map_with(|_, e| {
        Node::new(
            ExprKind::Path {
                path: Path::new(vec!["Some".into()]),
            },
            span(e.span()),
        )
    });
    let keyword_expr = just(Token::Colon)
        .ignore_then(fdn_name())
        .map_with(|name, e| Node::new(ExprKind::Keyword { name }, span(e.span())));
    let reader_expr = just(Token::Hash)
        .ignore_then(fdn_name())
        .then(fdn.clone())
        .map_with(|(tag, value), e| {
            Node::new(ExprKind::ReaderForm { tag, value }, span(e.span()))
        });
    let wrap_expr = just(Token::At)
        .ignore_then(select! { Token::Ident(name) if name == "wrap" => name })
        .then(expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .map_with(|(name, value), e| {
            Node::new(
                ExprKind::Annotated {
                    value: Box::new(value),
                    metadata: vec![Metadata {
                        name: Some(name),
                        arguments: Vec::new(),
                        map: None,
                    }],
                },
                span(e.span()),
            )
        });
    let init_field = ident()
        .then_ignore(just(Token::Colon))
        .then(expr.clone())
        .map(|(name, value)| InitField { name, value });
    let struct_init = path()
        .then(just(Token::ColonColon).ignore_then(ident()).or_not())
        .then(
            init_field
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|((namespace, variant), fields), e| {
            Node::new(
                ExprKind::StructInit {
                    namespace,
                    variant,
                    fields,
                    explicit_braces: true,
                },
                span(e.span()),
            )
        });
    let qualified_expr = path()
        .then_ignore(just(Token::ColonColon))
        .then(ident())
        .map_with(|(namespace, name), e| {
            Node::new(ExprKind::Qualified { namespace, name }, span(e.span()))
        });
    let path_expr = ident().map_with(|name, e| {
        Node::new(
            ExprKind::Path {
                path: Path::new(vec![name]),
            },
            span(e.span()),
        )
    });
    let array_expr = expr
        .clone()
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket))
        .map_with(|items, e| Node::new(ExprKind::Array { items }, span(e.span())));

    let closure_param = ident()
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .map(|(name, ty)| Param {
            name,
            ty,
            default: None,
        });
    let closure_params = closure_param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    let capture = choice((
        just(Token::Amp)
            .ignore_then(just(Token::Mut).or_not())
            .then(ident())
            .map(|(mutable, name)| Capture {
                name,
                by_reference: true,
                mutable: mutable.is_some(),
            }),
        ident().map(|name| Capture {
            name,
            by_reference: false,
            mutable: false,
        }),
    ));
    let capture_list = capture
        .separated_by(just(Token::Comma))
        .at_least(1)
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBracket), just(Token::RBracket));
    let closure_tail = closure_params
        .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
        .then(block.clone());
    let captured_closure = capture_list
        .then(closure_tail.clone())
        .map_with(|(captures, ((params, return_type), body)), e| {
            Node::new(
                ExprKind::Closure {
                    captures,
                    params,
                    return_type,
                    body,
                },
                span(e.span()),
            )
        });
    let capture_free_closure = closure_tail.map_with(|((params, return_type), body), e| {
        Node::new(
            ExprKind::Closure {
                captures: Vec::new(),
                params,
                return_type,
                body,
            },
            span(e.span()),
        )
    });

    let match_body = choice((
        block.clone().map(MatchBody::Block),
        expr.clone().map(MatchBody::Expr),
    ));
    let match_arm = pattern
        .clone()
        .then(just(Token::When).ignore_then(expr.clone()).or_not())
        .then_ignore(just(Token::FatArrow))
        .then(match_body)
        .then_ignore(choice((just(Token::Comma), just(Token::Semicolon))).or_not())
        .map(|((pattern, guard), body)| MatchArm {
            pattern,
            guard,
            body,
        });
    let match_expr = just(Token::Match)
        .ignore_then(expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(
            match_arm
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|(value, arms), e| {
            Node::new(
                ExprKind::Match {
                    value: Box::new(value),
                    arms,
                },
                span(e.span()),
            )
        });

    let parenthesized = expr
        .clone()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    let atom = choice((
        match_expr,
        wrap_expr,
        reader_expr,
        captured_closure,
        capture_free_closure,
        struct_init,
        qualified_expr,
        literal_expr,
        none_expr,
        some_expr,
        keyword_expr,
        array_expr,
        path_expr,
        parenthesized,
    ))
    .labelled("expression atom")
    .boxed();

    let named_arg = just(Token::Colon)
        .ignore_then(ident())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|(name, value)| CallArg::Named { name, value });
    let named_args = named_arg
        .separated_by(just(Token::Comma))
        .at_least(1)
        .allow_trailing()
        .collect::<Vec<_>>();
    let positional_args = expr
        .clone()
        .map(|value| CallArg::Positional { value })
        .separated_by(just(Token::Comma))
        .at_least(1)
        .allow_trailing()
        .collect::<Vec<_>>();
    let call_args = choice((named_args, positional_args, empty().to(Vec::<CallArg>::new())))
        .delimited_by(just(Token::LParen), just(Token::RParen));

    #[derive(Clone)]
    enum Postfix {
        Call(Vec<CallArg>),
        Index(Expr),
        Member(String),
        Try,
        Metadata(Metadata),
    }
    let postfix = choice((
        call_args.map(Postfix::Call),
        expr.clone()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map(Postfix::Index),
        just(Token::Dot).ignore_then(ident()).map(Postfix::Member),
        just(Token::Question).to(Postfix::Try),
        metadata.clone().map(Postfix::Metadata),
    ));
    let postfix_expr = atom.foldl_with(postfix.repeated(), |base, postfix, e| {
        let kind = match postfix {
            Postfix::Call(args) => ExprKind::Call {
                callee: Box::new(base),
                args,
            },
            Postfix::Index(index) => ExprKind::Index {
                base: Box::new(base),
                index: Box::new(index),
            },
            Postfix::Member(name) => ExprKind::Member {
                base: Box::new(base),
                name,
            },
            Postfix::Try => ExprKind::Try {
                value: Box::new(base),
            },
            Postfix::Metadata(item) => ExprKind::Annotated {
                value: Box::new(base),
                metadata: vec![item],
            },
        };
        Node::new(kind, span(e.span()))
    });
    let unary_op = choice((
        just(Token::Minus).to(UnaryOp::Neg),
        just(Token::Bang).to(UnaryOp::Not),
        just(Token::Tilde).to(UnaryOp::BitNot),
        just(Token::Amp).to(UnaryOp::AddressOf),
        just(Token::Star).to(UnaryOp::Deref),
    ));
    let unary = unary_op
        .repeated()
        .collect::<Vec<_>>()
        .then(postfix_expr)
        .map_with(|(operators, value), e| {
            let whole = span(e.span());
            operators.into_iter().rev().fold(value, |value, op| {
                Node::new(
                    ExprKind::Unary {
                        op,
                        value: Box::new(value),
                    },
                    whole,
                )
            })
        })
        .boxed();

    fn binary<'tokens, I, P, O>(
        term: P,
        operator: O,
    ) -> impl Parser<'tokens, I, Expr, ParseExtra<'tokens>> + Clone
    where
        I: ValueInput<'tokens, Token = Token, Span = CSpan>,
        P: Parser<'tokens, I, Expr, ParseExtra<'tokens>> + Clone,
        O: Parser<'tokens, I, BinaryOp, ParseExtra<'tokens>> + Clone,
    {
        term.clone()
            .foldl_with(operator.then(term).repeated(), |left, (op, right), e| {
                Node::new(
                    ExprKind::Binary {
                        op,
                        left: Box::new(left),
                        right: Box::new(right),
                    },
                    span(e.span()),
                )
            })
    }
    let product = binary(
        unary,
        choice((
            just(Token::Star).to(BinaryOp::Mul),
            just(Token::Slash).to(BinaryOp::Div),
            just(Token::Percent).to(BinaryOp::Rem),
        )),
    )
    .boxed();
    let sum = binary(
        product,
        choice((
            just(Token::Plus).to(BinaryOp::Add),
            just(Token::Minus).to(BinaryOp::Sub),
        )),
    )
    .boxed();
    let shift = binary(
        sum,
        choice((
            just(Token::ShiftLeft).to(BinaryOp::ShiftLeft),
            just(Token::ShiftRight).to(BinaryOp::ShiftRight),
        )),
    )
    .boxed();
    let compare = binary(
        shift,
        choice((
            just(Token::Less).to(BinaryOp::Less),
            just(Token::LessEq).to(BinaryOp::LessEq),
            just(Token::Greater).to(BinaryOp::Greater),
            just(Token::GreaterEq).to(BinaryOp::GreaterEq),
        )),
    )
    .boxed();
    let equality = binary(
        compare,
        choice((
            just(Token::EqEq).to(BinaryOp::Eq),
            just(Token::NotEq).to(BinaryOp::NotEq),
        )),
    )
    .boxed();
    let bit_and = binary(equality, just(Token::Amp).to(BinaryOp::BitAnd)).boxed();
    let bit_xor = binary(bit_and, just(Token::Caret).to(BinaryOp::BitXor)).boxed();
    let bit_or = binary(bit_xor, just(Token::Pipe).to(BinaryOp::BitOr)).boxed();
    let logical_and = binary(bit_or, just(Token::AndAnd).to(BinaryOp::LogicalAnd)).boxed();
    let logical_xor = binary(logical_and, just(Token::Xor).to(BinaryOp::LogicalXor)).boxed();
    expr.define(
        binary(logical_xor, just(Token::OrOr).to(BinaryOp::LogicalOr))
            .labelled("expression")
            .boxed(),
    );

    // Statements.
    let binding_kind = choice((
        just(Token::Val).to(BindingKind::Val),
        just(Token::Var).to(BindingKind::Var),
        just(Token::Const).to(BindingKind::Const),
    ));
    let value_decl = binding_kind
        .clone()
        .then(pattern.clone())
        .then(just(Token::Colon).ignore_then(ty.clone()).or_not())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|(((binding, pattern), ty), value)| ValueDecl {
            binding,
            pattern,
            ty,
            value,
        })
        .boxed();
    let assignment = expr
        .clone()
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .boxed();
    let value_stmt = value_decl
        .clone()
        .then_ignore(just(Token::Semicolon))
        .map_with(|value, e| Node::new(StmtKind::Value(value), span(e.span())));
    let assignment_stmt = assignment
        .clone()
        .then_ignore(just(Token::Semicolon))
        .map_with(|(target, value), e| {
            Node::new(StmtKind::Assignment { target, value }, span(e.span()))
        });
    let return_stmt = just(Token::Return)
        .ignore_then(just(Token::Tail).or_not())
        .then(expr.clone().or_not())
        .then_ignore(just(Token::Semicolon))
        .map_with(|(tail, value), e| {
            Node::new(
                StmtKind::Return {
                    tail: tail.is_some(),
                    value,
                },
                span(e.span()),
            )
        });
    let if_stmt = just(Token::If)
        .ignore_then(expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(block.clone())
        .then(just(Token::Else).ignore_then(stmt.clone()).or_not())
        .map_with(|((condition, then_block), else_branch), e| {
            Node::new(
                StmtKind::If {
                    condition,
                    then_block,
                    else_branch: else_branch.map(Box::new),
                },
                span(e.span()),
            )
        });
    let while_stmt = just(Token::While)
        .ignore_then(expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)))
        .then(block.clone())
        .map_with(|(condition, body), e| {
            Node::new(StmtKind::While { condition, body }, span(e.span()))
        });
    let foreach_binding = choice((
        just(Token::Val).to(BindingKind::Val),
        just(Token::Var).to(BindingKind::Var),
    ));
    let foreach_stmt = just(Token::For)
        .ignore_then(
            foreach_binding
                .then(pattern.clone())
                .then_ignore(just(Token::In))
                .then(expr.clone())
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(block.clone())
        .map_with(|(((binding, pattern), iterable), body), e| {
            Node::new(
                StmtKind::ForEach {
                    binding,
                    pattern,
                    iterable,
                    body,
                },
                span(e.span()),
            )
        });
    let for_init = choice((
        value_decl.clone().map(ForInit::Value),
        assignment
            .clone()
            .map(|(target, value)| ForInit::Assignment { target, value }),
        expr.clone().map(ForInit::Expr),
    ));
    let for_step = choice((
        assignment
            .clone()
            .map(|(target, value)| ForStep::Assignment { target, value }),
        expr.clone().map(ForStep::Expr),
    ));
    let c_for_stmt = just(Token::For)
        .ignore_then(
            for_init
                .or_not()
                .then_ignore(just(Token::Semicolon))
                .then(expr.clone().or_not())
                .then_ignore(just(Token::Semicolon))
                .then(for_step.or_not())
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(block.clone())
        .map_with(|(((init, condition), step), body), e| {
            Node::new(
                StmtKind::ForC {
                    init,
                    condition,
                    step,
                    body,
                },
                span(e.span()),
            )
        });
    let break_stmt = just(Token::Break)
        .then_ignore(just(Token::Semicolon))
        .map_with(|_, e| Node::new(StmtKind::Break, span(e.span())));
    let continue_stmt = just(Token::Continue)
        .then_ignore(just(Token::Semicolon))
        .map_with(|_, e| Node::new(StmtKind::Continue, span(e.span())));
    let defer_stmt = just(Token::Defer)
        .ignore_then(choice((
            block.clone().map(DeferBody::Block),
            expr.clone()
                .then_ignore(just(Token::Semicolon))
                .map(DeferBody::Expr),
        )))
        .map_with(|body, e| Node::new(StmtKind::Defer { body }, span(e.span())));
    let unsafe_stmt = just(Token::Unsafe)
        .ignore_then(block.clone())
        .map_with(|body, e| Node::new(StmtKind::Unsafe { body }, span(e.span())));
    let context_override = just(Token::Colon)
        .ignore_then(ident())
        .then_ignore(just(Token::Eq))
        .then(expr.clone())
        .map(|(name, value)| ContextOverride { name, value });
    let context_stmt = just(Token::With)
        .ignore_then(just(Token::Context))
        .ignore_then(
            context_override
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LParen), just(Token::RParen)),
        )
        .then(block.clone())
        .map_with(|(overrides, body), e| {
            Node::new(
                StmtKind::WithContext { overrides, body },
                span(e.span()),
            )
        });
    let receive_arm = just(Token::Recv)
        .ignore_then(expr.clone())
        .then_ignore(just(Token::Arrow))
        .then(pattern.clone())
        .then_ignore(just(Token::FatArrow))
        .then(block.clone())
        .map(|((channel, pattern), body)| SelectArm::Receive {
            channel,
            pattern,
            body,
        });
    let timeout_arm = just(Token::Timeout)
        .ignore_then(expr.clone())
        .then_ignore(just(Token::FatArrow))
        .then(block.clone())
        .map(|(duration, body)| SelectArm::Timeout { duration, body });
    let select_stmt = just(Token::Select)
        .ignore_then(
            choice((receive_arm, timeout_arm))
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|arms, e| Node::new(StmtKind::Select { arms }, span(e.span())));
    let nested_block = block
        .clone()
        .map_with(|block, e| Node::new(StmtKind::Block { block }, span(e.span())));
    let expr_stmt = expr
        .clone()
        .then_ignore(just(Token::Semicolon))
        .map_with(|expr, e| Node::new(StmtKind::Expr { expr }, span(e.span())));
    stmt.define(
        choice((
            value_stmt,
            return_stmt,
            if_stmt,
            while_stmt,
            foreach_stmt,
            c_for_stmt,
            break_stmt,
            continue_stmt,
            defer_stmt,
            unsafe_stmt,
            context_stmt,
            select_stmt,
            assignment_stmt,
            nested_block,
            expr_stmt,
        ))
        .recover_with(skip_then_retry_until(
            any().ignored(),
            choice((
                just(Token::Semicolon).ignored(),
                just(Token::RBrace).ignored(),
            )),
        ))
        .labelled("statement")
        .boxed(),
    );

    // Declarations.
    let fn_param = ident()
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .map(|(name, ty)| Param {
            name,
            ty,
            default: None,
        });
    let nfn_param = ident()
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .map(|((name, ty), default)| Param { name, ty, default });
    let fn_params = fn_param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    let nfn_params = nfn_param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));
    let function = choice((
        just(Token::Fn)
            .ignore_then(ident())
            .then(fn_params.clone())
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .then(block.clone())
            .map(|(((name, params), return_type), body)| {
                DeclKind::Function(FunctionDecl {
                    named_arguments: false,
                    name,
                    params,
                    return_type,
                    body,
                })
            }),
        just(Token::Nfn)
            .ignore_then(ident())
            .then(nfn_params.clone())
            .then(just(Token::Arrow).ignore_then(ty.clone()).or_not())
            .then(block.clone())
            .map(|(((name, params), return_type), body)| {
                DeclKind::Function(FunctionDecl {
                    named_arguments: true,
                    name,
                    params,
                    return_type,
                    body,
                })
            }),
    ))
    .boxed();
    let field = metadata
        .clone()
        .repeated()
        .collect::<Vec<_>>()
        .then(ident())
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .then_ignore(just(Token::Semicolon))
        .map(|(((metadata, name), ty), default)| FieldDecl {
            metadata,
            name,
            ty,
            default,
        })
        .boxed();
    let struct_decl = just(Token::Struct)
        .ignore_then(ident())
        .then(
            field
                .clone()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, fields)| DeclKind::Struct(StructDecl { name, fields }));
    let enum_item = ident()
        .then(just(Token::Eq).ignore_then(expr.clone()).or_not())
        .map(|(name, value)| EnumVariant { name, value });
    let enum_decl = just(Token::Enum)
        .ignore_then(ident())
        .then(
            enum_item
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| DeclKind::Enum(EnumDecl { name, variants }));
    let tagged_variant = ident()
        .then(
            field
                .clone()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .or_not(),
        )
        .map(|(name, fields)| TaggedVariant {
            name,
            fields: fields.unwrap_or_default(),
        });
    let tagged_decl = just(Token::Tagged)
        .ignore_then(ident())
        .then(
            tagged_variant
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(name, variants)| DeclKind::Tagged(TaggedDecl { name, variants }));
    let bit_field = ident()
        .then_ignore(just(Token::Colon))
        .then(select! { Token::Integer(text) => text })
        .then_ignore(just(Token::Semicolon))
        .map(|(name, text)| BitField {
            name,
            width: text.replace('_', "").parse().unwrap_or(0),
        });
    let bitstruct_decl = just(Token::BitStruct)
        .ignore_then(ident())
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then(
            bit_field
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|((name, storage), fields)| {
            DeclKind::BitStruct(BitStructDecl {
                name,
                storage,
                fields,
            })
        });
    let distinct_decl = just(Token::Distinct)
        .ignore_then(ident())
        .then_ignore(just(Token::Colon))
        .then(ty.clone())
        .then_ignore(just(Token::Semicolon))
        .map(|(name, underlying)| DeclKind::Distinct(DistinctDecl { name, underlying }));
    let alias_decl = just(Token::Type)
        .ignore_then(ident())
        .then_ignore(just(Token::Eq))
        .then(ty.clone())
        .then_ignore(just(Token::Semicolon))
        .map(|(name, target)| DeclKind::TypeAlias(TypeAliasDecl { name, target }));
    let impl_method = metadata
        .clone()
        .repeated()
        .collect::<Vec<_>>()
        .then(function.clone())
        .filter(|(_, kind)| matches!(kind, DeclKind::Function(_)))
        .map(|(metadata, kind)| {
            let DeclKind::Function(function) = kind else {
                unreachable!()
            };
            ImplMethod { metadata, function }
        });
    let impl_decl = just(Token::Impl)
        .ignore_then(path())
        .then(
            impl_method
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map(|(target, methods)| DeclKind::Impl(ImplDecl { target, methods }));
    let global = value_decl
        .clone()
        .then_ignore(just(Token::Semicolon))
        .map(DeclKind::Global);
    let declaration_kind = choice((
        function,
        struct_decl,
        enum_decl,
        tagged_decl,
        bitstruct_decl,
        distinct_decl,
        alias_decl,
        impl_decl,
        global,
    ))
    .boxed();
    let declaration = metadata
        .repeated()
        .collect::<Vec<_>>()
        .then(just(Token::Pub).or_not())
        .then(declaration_kind)
        .map_with(|((metadata, public), kind), e| {
            Node::new(
                DeclData {
                    public: public.is_some(),
                    metadata,
                    kind,
                },
                span(e.span()),
            )
        })
        .boxed();
    let imports = just(Token::Import)
        .ignore_then(
            path()
                .separated_by(just(Token::Comma))
                .at_least(1)
                .allow_trailing()
                .collect::<Vec<_>>(),
        )
        .then_ignore(just(Token::Semicolon));

    just(Token::Module)
        .ignore_then(path())
        .then_ignore(just(Token::Semicolon))
        .then(imports.repeated().collect::<Vec<_>>())
        .then(declaration.repeated().collect::<Vec<_>>())
        .then_ignore(end())
        .map(|((module, imports), declarations)| SourceFile {
            module,
            imports: imports.into_iter().flatten().collect(),
            declarations,
        })
        .boxed()
}

fn validate_source(file: &SourceFile, diagnostics: &mut Vec<Diagnostic>) {
    for declaration in &file.declarations {
        validate_decl(declaration, diagnostics);
    }
}

fn validate_decl(declaration: &Decl, diagnostics: &mut Vec<Diagnostic>) {
    match &declaration.kind.kind {
        DeclKind::Function(function) => validate_block(&function.body, diagnostics),
        DeclKind::Impl(imp) => {
            for method in &imp.methods {
                validate_block(&method.function.body, diagnostics);
            }
        }
        DeclKind::Global(value) => {
            validate_pattern(&value.pattern, diagnostics);
            validate_expr(&value.value, diagnostics);
        }
        DeclKind::Struct(value) => {
            for field in &value.fields {
                if let Some(default) = &field.default {
                    validate_expr(default, diagnostics);
                }
            }
        }
        DeclKind::Enum(value) => {
  if value.variants.is_empty() {
      diagnostics.push(Diagnostic {
          span: declaration.span,
          message: "enum declarations require at least one variant".into(),
      });
  }
  for variant in &value.variants {
                if let Some(value) = &variant.value {
                    validate_expr(value, diagnostics);
                }
            }
        }
        DeclKind::Tagged(value) => {
  if value.variants.is_empty() {
      diagnostics.push(Diagnostic {
          span: declaration.span,
          message: "tagged declarations require at least one variant".into(),
      });
  }
  for variant in &value.variants {
                for field in &variant.fields {
                    if let Some(default) = &field.default {
                        validate_expr(default, diagnostics);
                    }
                }
            }
        }
        DeclKind::BitStruct(_) | DeclKind::Distinct(_) | DeclKind::TypeAlias(_) => {}
    }
}

fn validate_block(block: &Block, diagnostics: &mut Vec<Diagnostic>) {
    for statement in &block.statements {
        match &statement.kind {
            StmtKind::Value(value) => {
                validate_pattern(&value.pattern, diagnostics);
                validate_expr(&value.value, diagnostics);
            }
            StmtKind::Assignment { target, value } => {
                if !is_assignable(target) {
                    diagnostics.push(Diagnostic {
                        span: target.span,
                        message: "left side of assignment is not assignable".into(),
                    });
                }
                validate_expr(target, diagnostics);
                validate_expr(value, diagnostics);
            }
            StmtKind::Expr { expr } => validate_expr(expr, diagnostics),
            StmtKind::Return { value, .. } => {
                if let Some(value) = value {
                    validate_expr(value, diagnostics);
                }
            }
            StmtKind::If {
                condition,
                then_block,
                else_branch,
            } => {
                validate_expr(condition, diagnostics);
                validate_block(then_block, diagnostics);
                if let Some(branch) = else_branch {
                    if !matches!(branch.kind, StmtKind::If { .. } | StmtKind::Block { .. }) {
                        diagnostics.push(Diagnostic {
                            span: branch.span,
                            message: "`else` must be followed by a block or `if`".into(),
                        });
                    }
                    validate_statement(branch, diagnostics);
                }
            }
            StmtKind::While { condition, body } => {
                validate_expr(condition, diagnostics);
                validate_block(body, diagnostics);
            }
            StmtKind::ForC {
                init,
                condition,
                step,
                body,
            } => {
                if let Some(init) = init {
                    match init {
                        ForInit::Value(value) => validate_expr(&value.value, diagnostics),
                        ForInit::Assignment { target, value } => {
                            if !is_assignable(target) {
                                diagnostics.push(Diagnostic {
                                    span: target.span,
                                    message: "left side of assignment is not assignable".into(),
                                });
                            }
                            validate_expr(target, diagnostics);
                            validate_expr(value, diagnostics);
                        }
                        ForInit::Expr(value) => validate_expr(value, diagnostics),
                    }
                }
                if let Some(condition) = condition {
                    validate_expr(condition, diagnostics);
                }
                if let Some(step) = step {
                    match step {
                        ForStep::Assignment { target, value } => {
                            if !is_assignable(target) {
                                diagnostics.push(Diagnostic {
                                    span: target.span,
                                    message: "left side of assignment is not assignable".into(),
                                });
                            }
                            validate_expr(target, diagnostics);
                            validate_expr(value, diagnostics);
                        }
                        ForStep::Expr(value) => validate_expr(value, diagnostics),
                    }
                }
                validate_block(body, diagnostics);
            }
            StmtKind::ForEach {
                pattern,
                iterable,
                body,
                ..
            } => {
                validate_pattern(pattern, diagnostics);
                validate_expr(iterable, diagnostics);
                validate_block(body, diagnostics);
            }
            StmtKind::Break | StmtKind::Continue => {}
            StmtKind::Defer { body } => match body {
                DeferBody::Block(block) => validate_block(block, diagnostics),
                DeferBody::Expr(expr) => validate_expr(expr, diagnostics),
            },
            StmtKind::Unsafe { body } | StmtKind::Block { block: body } => {
                validate_block(body, diagnostics)
            }
            StmtKind::WithContext { overrides, body } => {
                for item in overrides {
                    validate_expr(&item.value, diagnostics);
                }
                validate_block(body, diagnostics);
            }
            StmtKind::Select { arms } => {
                for arm in arms {
                    match arm {
                        SelectArm::Receive {
                            channel,
                            pattern,
                            body,
                        } => {
                            validate_expr(channel, diagnostics);
                            validate_pattern(pattern, diagnostics);
                            validate_block(body, diagnostics);
                        }
                        SelectArm::Timeout { duration, body } => {
                            validate_expr(duration, diagnostics);
                            validate_block(body, diagnostics);
                        }
                    }
                }
            }
        }
    }
}

fn validate_statement(statement: &Stmt, diagnostics: &mut Vec<Diagnostic>) {
    let block = Block {
        span: statement.span,
        statements: vec![statement.clone()],
    };
    validate_block(&block, diagnostics);
}

fn validate_expr(expr: &Expr, diagnostics: &mut Vec<Diagnostic>) {
    match &expr.kind {
        ExprKind::Array { items } => {
            for item in items {
                validate_expr(item, diagnostics);
            }
        }
        ExprKind::StructInit { fields, .. } => {
            for field in fields {
                validate_expr(&field.value, diagnostics);
            }
        }
        ExprKind::Unary { value, .. } | ExprKind::Try { value } => {
            validate_expr(value, diagnostics)
        }
        ExprKind::Binary { left, right, .. } => {
            validate_expr(left, diagnostics);
            validate_expr(right, diagnostics);
        }
        ExprKind::Call { callee, args } => {
            validate_expr(callee, diagnostics);
            for arg in args {
                match arg {
                    CallArg::Positional { value } | CallArg::Named { value, .. } => {
                        validate_expr(value, diagnostics)
                    }
                }
            }
        }
        ExprKind::Index { base, index } => {
            validate_expr(base, diagnostics);
            validate_expr(index, diagnostics);
        }
        ExprKind::Member { base, .. } => validate_expr(base, diagnostics),
        ExprKind::Closure { body, .. } => validate_block(body, diagnostics),
        ExprKind::Match { value, arms } => {
            validate_expr(value, diagnostics);
            for arm in arms {
                validate_pattern(&arm.pattern, diagnostics);
                if let Some(guard) = &arm.guard {
                    validate_expr(guard, diagnostics);
                }
                match &arm.body {
                    MatchBody::Block(block) => validate_block(block, diagnostics),
                    MatchBody::Expr(expr) => validate_expr(expr, diagnostics),
                }
            }
        }
        ExprKind::Annotated { value, .. } => validate_expr(value, diagnostics),
        ExprKind::Integer { .. }
        | ExprKind::Float { .. }
        | ExprKind::Character { .. }
        | ExprKind::String { .. }
        | ExprKind::CString { .. }
        | ExprKind::Bool { .. }
        | ExprKind::None
        | ExprKind::Keyword { .. }
        | ExprKind::Path { .. }
        | ExprKind::Qualified { .. }
        | ExprKind::ReaderForm { .. } => {}
    }
}

fn validate_pattern(pattern: &Pattern, diagnostics: &mut Vec<Diagnostic>) {
    match &pattern.kind {
        PatternKind::Variant { fields, .. } | PatternKind::Struct { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &field.pattern {
                    validate_pattern(pattern, diagnostics);
                }
            }
        }
        PatternKind::Sequence { items, .. } => {
            for item in items {
                validate_pattern(item, diagnostics);
            }
        }
        PatternKind::Some { value } | PatternKind::As { pattern: value, .. } => {
            validate_pattern(value, diagnostics)
        }
        PatternKind::Or { patterns } => {
            for pattern in patterns {
                validate_pattern(pattern, diagnostics);
            }
        }
        PatternKind::Wildcard
        | PatternKind::Binding { .. }
        | PatternKind::Literal { .. }
        | PatternKind::Range { .. }
        | PatternKind::None { .. }
        | PatternKind::Map { .. } => {}
    }
}

fn is_assignable(expr: &Expr) -> bool {
    matches!(
        expr.kind,
        ExprKind::Path { .. } | ExprKind::Member { .. } | ExprKind::Index { .. }
    ) || matches!(
        expr.kind,
        ExprKind::Unary {
            op: UnaryOp::Deref,
            ..
        }
    )
}

pub fn parse_source(source: &str) -> ParseOutput {
    let mut diagnostics = Vec::new();
    let tokens = Token::lexer(source)
        .spanned()
        .map(|(token, range)| {
            let token = match token {
                Ok(token) => token,
                Err(()) => {
                    diagnostics.push(Diagnostic {
                        span: Span::new(range.start, range.end),
                        message: format!("invalid token `{}`", &source[range.clone()]),
                    });
                    Token::Error
                }
            };
            (token, CSpan::from(range))
        })
        .collect::<Vec<_>>();
    let stream = Stream::from_iter(tokens)
        .map((0..source.len()).into(), |(token, span): (_, _)| (token, span));
    let (ast, parse_errors) = source_parser().parse(stream).into_output_errors();
    diagnostics.extend(parse_errors.into_iter().map(|error| Diagnostic {
        span: span(*error.span()),
        message: error.to_string(),
    }));
    if let Some(file) = &ast {
        validate_source(file, &mut diagnostics);
    }
    ParseOutput { ast, diagnostics }
}
