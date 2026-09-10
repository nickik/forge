#[path = "ast_v1.rs"]
pub mod ast;
pub mod lexer;
#[path = "parser_v1.rs"]
pub mod parser;

pub use parser::{parse_source, Diagnostic, ParseOutput};
