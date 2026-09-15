//! Mechanical FIR -> CLIF code-generation boundary.
//!
//! Forge language semantics end at FIR. This crate consumes verified FIR and
//! translates it to Cranelift IR without consulting AST, HIR, Typed HIR, or
//! reconstructing Forge-language decisions.

mod abi;
mod backend;
mod c9_memory_checks;
mod diagnostic;
#[allow(clippy::too_many_arguments)]
#[path = "function_c9d_impl.rs"]
mod function;
mod machine;
mod target;
mod types;

pub use backend::{CraneliftBackend, PreparedModule};
pub use diagnostic::BackendError;
pub use machine::MachineCode;
pub use target::{CraneliftTarget, TargetLayout};
pub use types::{ScalarLayout, TypeLowering};
