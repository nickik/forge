use forge_frontend::lexer::Token;
use logos::Logos;

#[test]
fn provider_identifiers_may_start_with_multiple_underscores() {
    let mut lexer = Token::lexer("__forge_console_write _ _hidden");
    assert_eq!(
        lexer
            .next()
            .expect("provider token")
            .expect("provider lexes"),
        Token::Ident("__forge_console_write".to_owned())
    );
    assert_eq!(
        lexer
            .next()
            .expect("wildcard token")
            .expect("wildcard lexes"),
        Token::Underscore
    );
    assert_eq!(
        lexer.next().expect("hidden token").expect("hidden lexes"),
        Token::Ident("_hidden".to_owned())
    );
    assert!(lexer.next().is_none());
}
