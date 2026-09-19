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
mod sia32_image;
mod sia32_object;
pub mod sia32_privileged;
mod sia32_shell;
mod target;
mod types;

pub use backend_c11a::{CraneliftBackend, PreparedModule};
pub use diagnostic::BackendError;
pub use globals::{
    GlobalInitialization, GlobalObjectPlan, GlobalObjectSymbol, GlobalStorageClass, PreparedGlobal,
    PreparedGlobals, PreparedStaticData, PreparedStaticRelocation,
};
pub use machine::{MachineCode, MachineRelocation};
pub use object::{NativeObject, ObjectLinkage, ObjectModulePlan, ObjectSymbol};
pub use sia32_image::{
    build_sia32_flat_image, Sia32ExecutableImage, Sia32ImageLayout, Sia32ImageRange,
    SIA32_DATA_ALIGNMENT, SIA32_TEXT_ALIGNMENT,
};
pub use sia32_object::{
    link_sia32_objects, link_sia32_sectioned_objects, LinkedSia32Object, Sia32Object,
    Sia32Relocation, Sia32Section, Sia32SectionBases, Sia32Symbol,
};
pub use sia32_shell::Sia32IntegrationShell;
pub use target::{CraneliftTarget, ExecutableFormat, TargetAbi, TargetLayout};
pub use types::{ScalarLayout, TypeLowering};
