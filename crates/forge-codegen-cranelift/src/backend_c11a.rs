use std::collections::BTreeMap;

use cranelift_codegen::ir::{Function, Signature};
use cranelift_codegen::Context;
use forge_fir::{DefId, FirModule, TypeDefinitionTable};
use target_lexicon::Triple;

use crate::backend_legacy;
use crate::{
    BackendError, CraneliftTarget, PreparedGlobal, PreparedGlobals, TargetLayout, TypeLowering,
};

/// C11a module preparation wraps the proven C10 function preparation with the
/// prepared-global state derived from the same verified FIR module.
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
        // Prepare and validate the complete global side of the module first.
        // This retains C9 layout, initializer classification/dependencies, and
        // the authoritative FIR initializer order in the prepared module.
        let globals = self.prepare_globals(module, definitions)?;

        // C11a deliberately does not lower LoadGlobal or emit initializer code.
        // Reuse the already-verified C10 function pipeline on the function
        // portion only; C11c/d will connect those remaining code-generation
        // paths without changing the prepared module model established here.
        let mut function_module = module.clone();
        function_module.globals.clear();
        function_module.global_initializers.clear();
        function_module.global_init_order.clear();
        let functions = self
            .legacy
            .prepare_module_with_types(&function_module, definitions)?;

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
