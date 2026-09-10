#[path = "ast_v1.rs"]
pub mod ast;
#[path = "body_hir_v1.rs"]
pub mod body_hir;
#[path = "hir_v1.rs"]
pub mod hir;
pub mod lexer;
#[path = "parser_v1.rs"]
pub mod parser;
#[path = "resolution_v1.rs"]
pub mod resolution;

pub use body_hir::{
    lower_resolved_bodies, BodyHirOutput, HirBody, HirExpr, HirExprKind, HirGlobalBody, HirLocalDecl,
    HirPattern, HirPatternKind, HirStmt, HirStmtKind, HirType, HirTypeKind,
};
pub use hir::{lower_module, DefId, HirDiagnostic, HirModule, HirOutput, Namespace};
pub use parser::{parse_source, Diagnostic, ParseOutput};
pub use resolution::{
    resolve_module_bodies, BodyResolutionOutput, HirLocal, LocalId, NameUse, ResolvedBody,
    ResolvedName,
};
