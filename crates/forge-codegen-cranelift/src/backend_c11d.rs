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

    /// Configure the real SIA32 target and its production Cranelift lowering
    /// path.
    pub fn sia32() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Sia32)
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
        let globals = self.prepare_globals_with_static_initializers(
            module,
            definitions,
            static_initializers,
        )?;
        let prepared = self
            .legacy
            .prepare_functions_and_initializers_with_globals(module, definitions)?;
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

    pub fn runtime_initializer_functions(&self) -> &BTreeMap<DefId, DefId> {
        &self.runtime_initializer_functions
    }

    pub fn runtime_initializer_function(&self, global: DefId) -> Option<DefId> {
        self.runtime_initializer_functions.get(&global).copied()
    }

    pub const fn module_initializer_owner(&self) -> Option<DefId> {
        self.module_initializer_owner
    }
}
