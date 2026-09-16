use std::collections::BTreeMap;

use cranelift_codegen::ir::{Function, Signature};
use cranelift_codegen::Context;
use forge_fir::{DefId, FirModule, StaticGlobalInitializerTable, TypeDefinitionTable};
use target_lexicon::Triple;

use crate::backend_legacy;
use crate::{
    BackendError, CraneliftTarget, PreparedGlobal, PreparedGlobals, TargetLayout, TypeLowering,
};

/// C11d module preparation compiles ordinary functions, runtime-global
/// initializer functions, and one synthetic module initializer through the
/// same C9/C11 lowering and ABI policy.
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
        // Context operations are explicit FIR. Give those semantic slots real
        // native storage only in the backend's private module view so source
        // identity and the public Forge ABI remain unchanged.
        let mut augmented = module.clone();
        crate::context::install_context_storage(&mut augmented)?;

        let globals = self.prepare_globals_with_static_initializers(
            &augmented,
            definitions,
            static_initializers,
        )?;
        let prepared = self
            .legacy
            .prepare_functions_and_initializers_with_globals(&augmented, definitions)?;
        Ok(PreparedModule {
            functions: prepared.module,
            globals,
            runtime_initializer_functions: prepared.initializer_functions,
            module_initializer_owner: prepared.module_initializer_owner,
        })
    }
}

pub struct PreparedModule {
    functions: backend_legacy::PreparedModule,
    globals: PreparedGlobals,
    runtime_initializer_functions: BTreeMap<DefId, DefId>,
    module_initializer_owner: Option<DefId>,
}

impl PreparedModule {
    pub const fn target(&self) -> CraneliftTarget {
        self.functions.target()
    }

    /// All emitted code functions. In C11d this includes ordinary Forge
    /// functions, internal runtime-global initializer functions, and the one
    /// synthetic module initializer when runtime initialization is required.
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

    /// Maps each runtime-initialized global to the deterministic internal
    /// function that computes its value.
    pub fn runtime_initializer_functions(&self) -> &BTreeMap<DefId, DefId> {
        &self.runtime_initializer_functions
    }

    pub fn runtime_initializer_function(&self, global: DefId) -> Option<DefId> {
        self.runtime_initializer_functions.get(&global).copied()
    }

    /// Synthetic function owner for the single module initialization entry
    /// point. It is `None` when the module has no runtime-initialized globals.
    /// As with other Forge functions, object linkage remains explicit: include
    /// this owner in `plan_object_module_with_exports`/`emit_object_with_exports`
    /// when an external startup routine must call it.
    pub const fn module_initializer_owner(&self) -> Option<DefId> {
        self.module_initializer_owner
    }
}
