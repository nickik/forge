#[path = "ast_v1.rs"]
pub mod ast;
#[path = "hir_v1.rs"]
pub mod hir;
pub mod lexer;
#[path = "parser_v1.rs"]
pub mod parser;

pub use hir::{lower_module, DefId, HirDiagnostic, HirModule, HirOutput, Namespace};
pub use parser::{parse_source, Diagnostic, ParseOutput};
