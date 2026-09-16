use std::collections::BTreeMap;

use cranelift_codegen::ir::{Function, Signature};
use cranelift_codegen::Context;
use forge_fir::{DefId, FirModule, StaticGlobalInitializerTable, TypeDefinitionTable};
use target_lexicon::Triple;

use crate::backend_legacy;
use crate::{
    BackendError, CraneliftTarget, PreparedGlobal, PreparedGlobals, TargetLayout, TypeLowering,
};

/// C11 module preparation keeps the function and global halves derived from
/// the same verified FIR module. C11c additionally lets function lowering see
/// the global table so `LoadGlobal` can produce real symbolic references.
pub struct CraneliftBackend {
    legacy: backend_legacy::CraneliftBackend,
}

impl CraneliftBackend {
    pub fn new(target: CraneliftTarget) -> Result<Self, BackendError> {
        Ok(Self {
            legacy: backend_legacy::CraneliftBackend::new(target)?,
        })
    }

    pub fn aarch64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Aarch64)
    }

    pub fn riscv64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Riscv64)
    }

    pub const fn target(&self) -> CraneliftTarget {
        self.legacy.target()
    }

    pub const fn target_layout(&self) -> &TargetLayout {
        self.legacy.target_layout()
    }

    pub fn type_lowering(&self) -> TypeLowering<'_> {
        self.legacy.type_lowering()
    }

    pub fn target_triple(&self) -> &Triple {
        self.legacy.target_triple()
    }

    pub fn new_context(&self) -> Context {
        self.legacy.new_context()
    }

    pub fn new_signature(&self) -> Signature {
        self.legacy.new_signature()
    }

    pub fn prepare_module(&self, module: &FirModule) -> Result<PreparedModule, BackendError> {
        let definitions = TypeDefinitionTable::new();
        self.prepare_module_with_types(module, &definitions)
    }

    pub fn prepare_module_with_types(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<PreparedModule, BackendError> {
        self.prepare_module_with_static_initializers(
            module,
            definitions,
            &StaticGlobalInitializerTable::new(),
        )
    }

    pub fn prepare_module_with_static_initializers(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
        static_initializers: &StaticGlobalInitializerTable,
    ) -> Result<PreparedModule, BackendError> {
        // Execution-context save/set/load/restore is already explicit FIR. C14
        // gives those semantic slots real native storage by augmenting only the
        // backend view of the module with five private zero-fill pointer slots.
        // Source/FIR identity and public ABI remain unchanged.
        let mut augmented = module.clone();
        crate::context::install_context_storage(&mut augmented)?;
        let globals = self.prepare_globals_with_static_initializers(
            &augmented,
            definitions,
            static_initializers,
        )?;
        let functions = self
            .legacy
            .prepare_functions_with_globals(&augmented, definitions)?;
        Ok(PreparedModule { functions, globals })
    }
}

pub struct PreparedModule {
    functions: backend_legacy::PreparedModule,
    globals: PreparedGlobals,
}

impl PreparedModule {
    pub const fn target(&self) -> CraneliftTarget {
        self.functions.target()
    }

    pub fn functions(&self) -> &BTreeMap<DefId, Function> {
        self.functions.functions()
    }

    pub fn function(&self, owner: DefId) -> Option<&Function> {
        self.functions.function(owner)
    }

    pub fn prepared_globals(&self) -> &PreparedGlobals {
        &self.globals
    }

    pub fn globals(&self) -> &BTreeMap<DefId, PreparedGlobal> {
        self.globals.globals()
    }

    pub fn global(&self, owner: DefId) -> Option<&PreparedGlobal> {
        self.globals.global(owner)
    }

    pub fn global_init_order(&self) -> &[DefId] {
        self.globals.init_order()
    }
}
