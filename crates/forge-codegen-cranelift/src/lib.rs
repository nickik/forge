//! Mechanical FIR -> CLIF code-generation boundary.
//!
//! Forge language semantics end at FIR. This crate consumes verified FIR and
//! translates it to Cranelift IR without consulting AST, HIR, Typed HIR, or
//! reconstructing Forge-language decisions.

#[allow(dead_code)]
mod abi;
mod backend;
mod c9_memory_checks;
mod diagnostic;
#[allow(clippy::too_many_arguments, dead_code)]
#[path = "function_c9d_impl.rs"]
mod function;
mod machine;
mod object;
mod target;
mod types;

pub use backend::{CraneliftBackend, PreparedModule};
pub use diagnostic::BackendError;
pub use machine::MachineCode;
pub use object::{NativeObject, ObjectLinkage, ObjectModulePlan, ObjectSymbol};
pub use target::{CraneliftTarget, TargetLayout};
pub use types::{ScalarLayout, TypeLowering};
