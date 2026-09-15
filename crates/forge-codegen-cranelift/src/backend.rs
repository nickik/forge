use std::collections::BTreeMap;

use cranelift_codegen::ir::{Function, Signature};
use cranelift_codegen::isa::{CallConv, OwnedTargetIsa};
use cranelift_codegen::Context;
use forge_fir::{verify_fir_module, DefId, FirModule};
use target_lexicon::Triple;

use crate::function::lower_function;
use crate::{BackendError, CraneliftTarget, TargetLayout, TypeLowering};

/// Target-specific Cranelift state. It deliberately owns no Forge semantic
/// state other than verified FIR passed to lowering operations.
pub struct CraneliftBackend {
    target: CraneliftTarget,
    layout: TargetLayout,
    isa: OwnedTargetIsa,
}

impl CraneliftBackend {
    pub fn new(target: CraneliftTarget) -> Result<Self, BackendError> {
        let isa = target.isa()?;
        Ok(Self {
            target,
            layout: target.layout(),
            isa,
        })
    }

    pub fn aarch64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Aarch64)
    }

    pub fn riscv64() -> Result<Self, BackendError> {
        Self::new(CraneliftTarget::Riscv64)
    }

    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub const fn target_layout(&self) -> &TargetLayout {
        &self.layout
    }

    pub fn type_lowering(&self) -> TypeLowering<'_> {
        TypeLowering::new(&self.layout)
    }

    pub fn target_triple(&self) -> &Triple {
        self.isa.triple()
    }

    pub fn new_context(&self) -> Context {
        Context::new()
    }

    pub fn new_signature(&self) -> Signature {
        Signature::new(CallConv::triple_default(self.isa.triple()))
    }

    /// Verify FIR and mechanically lower every supported FIR function to CLIF.
    /// Globals and runtime initializers remain explicit unsupported boundaries.
    pub fn prepare_module(&self, module: &FirModule) -> Result<PreparedModule, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        if !module.globals.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "globals",
            });
        }
        if !module.global_initializers.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "global initializers",
            });
        }
        if !module.global_init_order.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "global initializer order",
            });
        }

        let lowering = self.type_lowering();
        let mut functions = BTreeMap::new();
        for (owner, fir) in &module.functions {
            let function = lower_function(fir, &lowering, &*self.isa)?;
            functions.insert(*owner, function);
        }

        Ok(PreparedModule {
            target: self.target,
            functions,
        })
    }
}

pub struct PreparedModule {
    target: CraneliftTarget,
    functions: BTreeMap<DefId, Function>,
}

impl PreparedModule {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn functions(&self) -> &BTreeMap<DefId, Function> {
        &self.functions
    }

    pub fn function(&self, owner: DefId) -> Option<&Function> {
        self.functions.get(&owner)
    }
}
