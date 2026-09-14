#[path = "ast_v1.rs"]
pub mod ast;
#[path = "body_hir_v1.rs"]
pub mod body_hir;
#[path = "fir_v1.rs"]
pub mod fir;
#[path = "hir_v1.rs"]
pub mod hir;
pub mod lexer;
#[path = "parser_v1.rs"]
pub mod parser;
#[path = "resolution_v1.rs"]
pub mod resolution;
#[path = "typecheck_v1.rs"]
pub mod typecheck;

pub use body_hir::{
    lower_resolved_bodies, BodyHirOutput, ExprId, HirBody, HirExpr, HirExprKind, HirGlobalBody,
    HirLocalDecl, HirPattern, HirPatternKind, HirStmt, HirStmtKind, HirType, HirTypeKind,
};
pub use hir::{
    lower_module, DefId, HirDiagnostic, HirMethod, HirModule, HirOutput, MetadataTable,
    MetadataTableExt, MetadataTarget, Namespace,
};
pub use parser::{parse_source, Diagnostic, ParseOutput};
pub use resolution::{
    resolve_module_bodies, BodyResolutionOutput, HirLocal, LocalId, NameUse, ResolvedBody,
    ResolvedName,
};
pub use typecheck::{
    type_check_module, CaptureMode, ConstValue, ContextSlot, IntWidth, MatchCondition,
    MatchProjection, MatchScalar, MatchTest, ResolvedCallArgument, ResolvedReceiver, Ty,
    TypeCheckOutput, TypeDiagnostic, TypedBody, TypedCapture, TypedClosurePlan,
    TypedContextOverride, TypedContextScope, TypedExpr, TypedExprKind, TypedMatchArmPlan,
    TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,
};

pub use fir::{
    lower_fir, verify_fir_function, FirBasicBlock, FirBlockId, FirConst, FirDiagnostic,
    FirFunction, FirGlobal, FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule,
    FirOutput, FirPlace, FirSelectCase, FirTerminator, FirUnaryOp, FirValueId, OverflowMode,
};

pub use typecheck::{RuntimeOperationId, TypedSelectArm, TypedSelectPlan};
