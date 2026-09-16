//! Mechanical FIR -> CLIF code-generation boundary.
//!
//! Forge language semantics end at FIR. This crate consumes verified FIR and
//! translates it to Cranelift IR without consulting AST, HIR, Typed HIR, or
//! reconstructing Forge-language decisions.

#[allow(dead_code)]
mod abi;
#[path = "backend_c11d.rs"]
mod backend_c11a;
#[allow(dead_code)]
#[path = "backend_c11d_legacy.rs"]
mod backend_legacy;
mod c9_memory_checks;
mod completion;
mod context;
mod diagnostic;
#[allow(clippy::too_many_arguments, dead_code)]
#[path = "function_c11c_impl.rs"]
mod function;
#[path = "globals_c11b.rs"]
mod globals;
mod machine;
#[allow(clippy::too_many_arguments)]
#[path = "object_c11c.rs"]
mod object;
mod target;
mod types;

pub use backend_c11a::{CraneliftBackend, PreparedModule};
pub use diagnostic::BackendError;
pub use globals::{
    GlobalInitialization, GlobalObjectPlan, GlobalObjectSymbol, GlobalStorageClass, PreparedGlobal,
    PreparedGlobals, PreparedStaticData, PreparedStaticRelocation,
};
pub use machine::MachineCode;
pub use object::{NativeObject, ObjectLinkage, ObjectModulePlan, ObjectSymbol};
pub use target::{CraneliftTarget, TargetLayout};
pub use types::{ScalarLayout, TypeLowering};
