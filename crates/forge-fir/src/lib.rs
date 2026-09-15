//! Frozen code-generation-facing Forge Intermediate Representation surface.
//!
//! Backends depend on this crate rather than `forge-frontend` directly. The
//! facade intentionally exposes FIR plus only the foundational IDs/types that
//! occur in FIR fields. HIR, typed-HIR, parser, resolution, and type-checker
//! implementation structures are not part of the backend API.

#[path = "layout_c9a.rs"]
mod layout;

pub use forge_frontend::ast::{BinaryOp, Span};
pub use forge_frontend::{
    collect_type_definitions, dump_fir_module, verify_fir_function, verify_fir_module, CaptureMode,
    ConstValue, ContextSlot, DefId, ExprId, FirBasicBlock, FirBlockId, FirConst, FirDiagnostic,
    FirFunction, FirGlobal, FirGlobalInitializer, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirOutput, FirPlace, FirSelectCase, FirTerminator, FirUnaryOp,
    FirValueId, IntWidth, LocalId, OverflowMode, RuntimeOperationId, Ty, TypeDefinition,
    TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition, TypeVariantDefinition,
    UnsafeOperationKind, UnsafeProvenance,
};
pub use layout::{
    FieldLayout, Layout, LayoutEngine, LayoutError, LayoutKind, LayoutTarget, Niche, SumEncoding,
    TagLayout, VariantLayout,
};
