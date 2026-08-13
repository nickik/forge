use chumsky::{
    input::{Stream, ValueInput},
    prelude::*,
};
use logos::Logos;

use crate::ast::*;
use crate::lexer::Token;

pub type CSpan = SimpleSpan<usize>;

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

fn span(s: CSpan) -> Span {
    Span::new(s.start, s.end)
}

fn path_parser<'tokens, I>() -> impl Parser<'tokens, I, Path, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    select! { Token::Ident(name) => name }
        .separated_by(just(Token::Dot))
        .at_least(1)
        .collect::<Vec<_>>()
        .map(Path::new)
        .labelled("qualified name")
}

fn type_parser<'tokens, I>() -> impl Parser<'tokens, I, TypeNode, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    recursive(|ty| {
        let named = path_parser()
            .map_with(|path, e| Node::new(TypeKind::Named { path }, span(e.span())));

        let result = just(Token::ResultType)
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

        let fixed_array = ty
            .clone()
            .then_ignore(just(Token::Semicolon))
            .then(expr_parser())
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

        let function_type = just(Token::Fn)
            .ignore_then(
                ty.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
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
            .ignore_then(
                ty.clone()
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LParen), just(Token::RParen)),
            )
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

        let base = choice((result, fixed_array, function_type, closure_type, named));

        #[derive(Clone)]
        enum Prefix {
            Pointer,
            Reference(bool),
        }

        let prefixes = choice((
            just(Token::Star).to(Prefix::Pointer),
            just(Token::Amp)
                .ignore_then(just(Token::Mut).or_not())
                .map(|m| Prefix::Reference(m.is_some())),
        ))
        .repeated();

        // Prefixes bind before postfix `?`/`[]`, so `&User?` means Option[&User].
        let prefixed = prefixes.foldr_with(base, |prefix, inner, e| {
            let kind = match prefix {
                Prefix::Pointer => TypeKind::Pointer {
                    inner: Box::new(inner),
                },
                Prefix::Reference(mutable) => TypeKind::Reference {
                    mutable,
                    inner: Box::new(inner),
                },
            };
            Node::new(kind, span(e.span()))
        });

        #[derive(Clone)]
        enum Suffix {
            Optional,
            Slice(bool),
        }

        let suffix = choice((
            just(Token::Question).to(Suffix::Optional),
            just(Token::LBracket)
                .ignore_then(just(Token::RBracket))
                .ignore_then(just(Token::Mut).or_not())
                .map(|m| Suffix::Slice(m.is_some())),
        ));

        prefixed.foldl_with(suffix.repeated(), |base, suffix, e| {
            let kind = match suffix {
                Suffix::Optional => TypeKind::Optional {
                    inner: Box::new(base),
                },
                Suffix::Slice(mutable) => TypeKind::Slice {
                    mutable,
                    element: Box::new(base),
                },
            };
            Node::new(kind, span(e.span()))
        })
    })
}

fn expr_parser<'tokens, I>() -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    recursive(|expr| {
        let literal = select! {
            Token::Integer(text) => ExprKind::Integer { text },
            Token::Float(text) => ExprKind::Float { text },
            Token::String(value) => ExprKind::String { value },
            Token::True => ExprKind::Bool { value: true },
            Token::False => ExprKind::Bool { value: false },
        }
        .map_with(|kind, e| Node::new(kind, span(e.span())));

        let path = path_parser().map_with(|path, e| Node::new(ExprKind::Path { path }, span(e.span())));

        let init_field = select! { Token::Ident(name) => name }
            .then_ignore(just(Token::Colon))
            .then(expr.clone())
            .map(|(name, value)| InitField { name, value });

        let struct_init = path_parser()
            .then(
                init_field
                    .separated_by(just(Token::Comma))
                    .allow_trailing()
                    .collect::<Vec<_>>()
                    .delimited_by(just(Token::LBrace), just(Token::RBrace)),
            )
            .map_with(|(ty, fields), e| Node::new(ExprKind::StructInit { ty, fields }, span(e.span())));

        let array = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBracket), just(Token::RBracket))
            .map_with(|items, e| Node::new(ExprKind::Array { items }, span(e.span())));

        let atom = choice((
            struct_init,
            literal,
            array,
            path,
            expr.clone().delimited_by(just(Token::LParen), just(Token::RParen)),
        ))
        .labelled("expression atom")
        .boxed();

        let args = expr
            .clone()
            .separated_by(just(Token::Comma))
            .allow_trailing()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LParen), just(Token::RParen));

        #[derive(Clone)]
        enum Postfix {
            Call(Vec<Expr>),
            Index(Expr),
        }

        let postfix = choice((
            args.map(Postfix::Call),
            expr.clone()
                .delimited_by(just(Token::LBracket), just(Token::RBracket))
                .map(Postfix::Index),
        ));

        let postfix_expr = atom.foldl_with(postfix.repeated(), |base, op, e| {
            let kind = match op {
                Postfix::Call(args) => ExprKind::Call { callee: Box::new(base), args },
                Postfix::Index(index) => ExprKind::Index { base: Box::new(base), index: Box::new(index) },
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
            .map_with(|(ops, value), e| {
                let whole = span(e.span());
                ops.into_iter().rev().fold(value, |value, op| {
                    Node::new(ExprKind::Unary { op, value: Box::new(value) }, whole)
                })
            });

        fn bin<'tokens, I, P, O>(
            lhs: P,
            op: O,
        ) -> impl Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
        where
            I: ValueInput<'tokens, Token = Token, Span = CSpan>,
            P: Parser<'tokens, I, Expr, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone,
            O: Parser<'tokens, I, BinaryOp, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone,
        {
            lhs.clone().foldl_with(op.then(lhs).repeated(), |left, (op, right), e| {
                Node::new(
                    ExprKind::Binary { op, left: Box::new(left), right: Box::new(right) },
                    span(e.span()),
                )
            })
        }

        let product = bin(
            unary,
            choice((
                just(Token::Star).to(BinaryOp::Mul),
                just(Token::Slash).to(BinaryOp::Div),
                just(Token::Percent).to(BinaryOp::Rem),
            )),
        );
        let sum = bin(
            product,
            choice((
                just(Token::Plus).to(BinaryOp::Add),
                just(Token::Minus).to(BinaryOp::Sub),
            )),
        );
        let shift = bin(
            sum,
            choice((
                just(Token::ShiftLeft).to(BinaryOp::ShiftLeft),
                just(Token::ShiftRight).to(BinaryOp::ShiftRight),
            )),
        );
        let compare = bin(
            shift,
            choice((
                just(Token::Less).to(BinaryOp::Less),
                just(Token::LessEq).to(BinaryOp::LessEq),
                just(Token::Greater).to(BinaryOp::Greater),
                just(Token::GreaterEq).to(BinaryOp::GreaterEq),
            )),
        );
        let equality = bin(
            compare,
            choice((
                just(Token::EqEq).to(BinaryOp::Eq),
                just(Token::NotEq).to(BinaryOp::NotEq),
            )),
        );
        let bit_and = bin(equality, just(Token::Amp).to(BinaryOp::BitAnd));
        let bit_xor = bin(bit_and, just(Token::Caret).to(BinaryOp::BitXor));
        let bit_or = bin(bit_xor, just(Token::Pipe).to(BinaryOp::BitOr));
        let logical_and = bin(bit_or, just(Token::AndAnd).to(BinaryOp::LogicalAnd));
        let logical_xor = bin(logical_and, just(Token::Xor).to(BinaryOp::LogicalXor));
        bin(logical_xor, just(Token::OrOr).to(BinaryOp::LogicalOr)).labelled("expression")
    })
}

fn value_decl_parser<'tokens, I>() -> impl Parser<'tokens, I, ValueDecl, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    choice((
        just(Token::Val).to(BindingKind::Val),
        just(Token::Var).to(BindingKind::Var),
        just(Token::Const).to(BindingKind::Const),
    ))
    .then(select! { Token::Ident(name) => name })
    .then(just(Token::Colon).ignore_then(type_parser()).or_not())
    .then_ignore(just(Token::Eq))
    .then(expr_parser())
    .map(|(((binding, name), ty), value)| ValueDecl { binding, name, ty, value })
}

fn block_parser<'tokens, I>() -> impl Parser<'tokens, I, Block, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    statement_parser()
        .repeated()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LBrace), just(Token::RBrace))
        .map_with(|statements, e| Block { span: span(e.span()), statements })
}

fn statement_parser<'tokens, I>() -> impl Parser<'tokens, I, Stmt, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    recursive(|stmt| {
        let block = stmt
            .clone()
            .repeated()
            .collect::<Vec<_>>()
            .delimited_by(just(Token::LBrace), just(Token::RBrace))
            .map_with(|statements, e| Block { span: span(e.span()), statements });

        let value = value_decl_parser()
            .then_ignore(just(Token::Semicolon))
            .map_with(|decl, e| Node::new(StmtKind::Value(decl), span(e.span())));

        let ret = just(Token::Return)
            .ignore_then(just(Token::Tail).or_not())
            .then(expr_parser().or_not())
            .then_ignore(just(Token::Semicolon))
            .map_with(|(tail, value), e| {
                Node::new(StmtKind::Return { tail: tail.is_some(), value }, span(e.span()))
            });

        let if_stmt = just(Token::If)
            .ignore_then(expr_parser().delimited_by(just(Token::LParen), just(Token::RParen)))
            .then(block.clone())
            .then(just(Token::Else).ignore_then(block.clone()).or_not())
            .map_with(|((condition, then_block), else_block), e| {
                Node::new(StmtKind::If { condition, then_block, else_block }, span(e.span()))
            });

        let while_stmt = just(Token::While)
            .ignore_then(expr_parser().delimited_by(just(Token::LParen), just(Token::RParen)))
            .then(block.clone())
            .map_with(|(condition, body), e| Node::new(StmtKind::While { condition, body }, span(e.span())));

        let defer_stmt = just(Token::Defer)
            .ignore_then(block.clone())
            .map_with(|body, e| Node::new(StmtKind::Defer { body }, span(e.span())));

        let unsafe_stmt = just(Token::Unsafe)
            .ignore_then(block.clone())
            .map_with(|body, e| Node::new(StmtKind::Unsafe { body }, span(e.span())));

        let nested_block = block
            .clone()
            .map_with(|block, e| Node::new(StmtKind::Block { block }, span(e.span())));

        let expr_stmt = expr_parser()
            .then_ignore(just(Token::Semicolon))
            .map_with(|expr, e| Node::new(StmtKind::Expr { expr }, span(e.span())));

        choice((value, ret, if_stmt, while_stmt, defer_stmt, unsafe_stmt, nested_block, expr_stmt))
            .recover_with(skip_then_retry_until(
                any().ignored(),
                choice((just(Token::Semicolon).ignored(), just(Token::RBrace).ignored())),
            ))
    })
}

fn field_parser<'tokens, I>() -> impl Parser<'tokens, I, FieldDecl, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    select! { Token::Ident(name) => name }
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .then_ignore(just(Token::Semicolon))
        .map(|(name, ty)| FieldDecl { name, ty })
}

fn declaration_parser<'tokens, I>() -> impl Parser<'tokens, I, Decl, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    let visibility = just(Token::Pub).or_not().map(|x| x.is_some());
    let ident = select! { Token::Ident(name) => name };

    let param = ident
        .clone()
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .then(just(Token::Eq).ignore_then(expr_parser()).or_not())
        .map(|((name, ty), default)| Param { name, ty, default });

    let params = param
        .separated_by(just(Token::Comma))
        .allow_trailing()
        .collect::<Vec<_>>()
        .delimited_by(just(Token::LParen), just(Token::RParen));

    let function = visibility
        .clone()
        .then(choice((just(Token::Fn).to(false), just(Token::Nfn).to(true))))
        .then(ident.clone())
        .then(params)
        .then(just(Token::Arrow).ignore_then(type_parser()).or_not())
        .then(block_parser())
        .map_with(|(((((public, named_arguments), name), params), return_type), body), e| {
            Node::new(
                DeclKind::Function(FunctionDecl { public, named_arguments, name, params, return_type, body }),
                span(e.span()),
            )
        });

    let struct_decl = visibility
        .clone()
        .then_ignore(just(Token::Struct))
        .then(ident.clone())
        .then(field_parser().repeated().collect::<Vec<_>>().delimited_by(just(Token::LBrace), just(Token::RBrace)))
        .map_with(|((public, name), fields), e| {
            Node::new(DeclKind::Struct(StructDecl { public, name, fields }), span(e.span()))
        });

    let enum_decl = visibility
        .clone()
        .then_ignore(just(Token::Enum))
        .then(ident.clone())
        .then(
            ident.clone()
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|((public, name), variants), e| {
            Node::new(DeclKind::Enum(EnumDecl { public, name, variants }), span(e.span()))
        });

    let tagged_variant = ident
        .clone()
        .then(
            field_parser()
                .repeated()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace))
                .or_not(),
        )
        .map(|(name, fields)| TaggedVariant { name, fields: fields.unwrap_or_default() });

    let tagged_decl = visibility
        .clone()
        .then_ignore(just(Token::Tagged))
        .then(ident.clone())
        .then(
            tagged_variant
                .separated_by(just(Token::Comma))
                .allow_trailing()
                .collect::<Vec<_>>()
                .delimited_by(just(Token::LBrace), just(Token::RBrace)),
        )
        .map_with(|((public, name), variants), e| {
            Node::new(DeclKind::Tagged(TaggedDecl { public, name, variants }), span(e.span()))
        });

    let distinct = visibility
        .clone()
        .then_ignore(just(Token::Distinct))
        .then(ident.clone())
        .then_ignore(just(Token::Colon))
        .then(type_parser())
        .then_ignore(just(Token::Semicolon))
        .map_with(|((public, name), underlying), e| {
            Node::new(DeclKind::Distinct(DistinctDecl { public, name, underlying }), span(e.span()))
        });

    let alias = visibility
        .clone()
        .then_ignore(just(Token::Type))
        .then(ident.clone())
        .then_ignore(just(Token::Eq))
        .then(type_parser())
        .then_ignore(just(Token::Semicolon))
        .map_with(|((public, name), target), e| {
            Node::new(DeclKind::TypeAlias(TypeAliasDecl { public, name, target }), span(e.span()))
        });

    let global = visibility
        .then(value_decl_parser())
        .then_ignore(just(Token::Semicolon))
        .map_with(|(public, value), e| {
            Node::new(DeclKind::Global { public, value }, span(e.span()))
        });

    choice((function, struct_decl, enum_decl, tagged_decl, distinct, alias, global))
        .labelled("declaration")
}

fn source_file_parser<'tokens, I>() -> impl Parser<'tokens, I, SourceFile, extra::Err<Rich<'tokens, Token, CSpan>>> + Clone
where
    I: ValueInput<'tokens, Token = Token, Span = CSpan>,
{
    just(Token::Module)
        .ignore_then(path_parser())
        .then_ignore(just(Token::Semicolon))
        .then(
            just(Token::Import)
                .ignore_then(path_parser())
                .then_ignore(just(Token::Semicolon))
                .repeated()
                .collect::<Vec<_>>(),
        )
        .then(declaration_parser().repeated().collect::<Vec<_>>())
        .then_ignore(end())
        .map(|((module, imports), declarations)| SourceFile { module, imports, declarations })
}

pub fn parse_source(source: &str) -> ParseOutput {
    let mut diagnostics = Vec::new();

    let token_iter = Token::lexer(source).spanned().map(|(token, range)| {
        let token = match token {
            Ok(token) => token,
            Err(()) => {
                diagnostics.push(Diagnostic {
                    span: Span::new(range.start, range.end),
                    message: format!("invalid token `{}`", &source[range.clone()]),
                });
                // Keep a real error token in the stream so parser recovery can continue
                // without accidentally treating invalid source as a valid identifier.
                Token::Error
            }
        };
        (token, CSpan::from(range))
    });

    let stream = Stream::from_iter(token_iter)
        .map((0..source.len()).into(), |(token, span): (_, _)| (token, span));

    let (ast, parse_errors) = source_file_parser().parse(stream).into_output_errors();
    diagnostics.extend(parse_errors.into_iter().map(|error| Diagnostic {
        span: span(*error.span()),
        message: error.to_string(),
    }));

    ParseOutput { ast, diagnostics }
}
