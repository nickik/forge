//! Mechanical FIR -> CLIF code-generation boundary.
//!
//! Forge language semantics end at FIR. This crate may consume verified FIR and
//! translate it to Cranelift IR, but it must not inspect or reconstruct source,
//! HIR, type-checker, pattern, capture, default-argument, or safety semantics.

use std::fmt;
use std::str::FromStr;

use cranelift_codegen::ir::Signature;
use cranelift_codegen::isa::{self, CallConv, OwnedTargetIsa};
use cranelift_codegen::{settings, Context};
use forge_fir::{verify_fir_module, FirModule};
use target_lexicon::Triple;

/// The targets used to validate Forge's target-independent FIR -> CLIF lowering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CraneliftTarget {
    Aarch64,
    Riscv64,
}

impl CraneliftTarget {
    pub const fn triple(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64-unknown-linux-gnu",
            Self::Riscv64 => "riscv64gc-unknown-linux-gnu",
        }
    }
}

/// Backend failures are code-generation failures only. They must never be used
/// to defer or re-run Forge semantic analysis below FIR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackendError {
    InvalidFir { diagnostic_count: usize },
    UnsupportedFir { component: &'static str },
    InvalidTarget { triple: &'static str, message: String },
    Cranelift { message: String },
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidFir { diagnostic_count } => {
                write!(f, "FIR verification failed with {diagnostic_count} diagnostic(s)")
            }
            Self::UnsupportedFir { component } => {
                write!(f, "FIR component is not lowered to CLIF yet: {component}")
            }
            Self::InvalidTarget { triple, message } => {
                write!(f, "invalid Cranelift target {triple}: {message}")
            }
            Self::Cranelift { message } => write!(f, "Cranelift error: {message}"),
        }
    }
}

impl std::error::Error for BackendError {}

/// Target-specific Cranelift state. It deliberately owns no Forge semantic
/// state other than the verified FIR passed to individual operations.
pub struct CraneliftBackend {
    target: CraneliftTarget,
    isa: OwnedTargetIsa,
}

impl CraneliftBackend {
    pub fn new(target: CraneliftTarget) -> Result<Self, BackendError> {
        let triple = Triple::from_str(target.triple()).map_err(|error| BackendError::InvalidTarget {
            triple: target.triple(),
            message: error.to_string(),
        })?;
        let flags = settings::Flags::new(settings::builder());
        let builder = isa::lookup(triple).map_err(|error| BackendError::InvalidTarget {
            triple: target.triple(),
            message: error.to_string(),
        })?;
        let isa = builder
            .finish(flags)
            .map_err(|error| BackendError::Cranelift {
                message: error.to_string(),
            })?;

        Ok(Self { target, isa })
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

    pub fn target_triple(&self) -> &Triple {
        self.isa.triple()
    }

    /// Create an empty Cranelift compilation context. Function population starts
    /// in C2/C3 when FIR types and CFG operations receive mechanical lowering.
    pub fn new_context(&self) -> Context {
        Context::new()
    }

    /// Create a target-ABI signature shell. Parameter/result population belongs
    /// to the FIR type-lowering milestone rather than C1.
    pub fn new_signature(&self) -> Signature {
        Signature::new(CallConv::triple_default(self.isa.triple()))
    }

    /// Validate the FIR/CLIF boundary and prepare Cranelift state.
    ///
    /// C1 intentionally accepts only an empty module. Every non-empty FIR
    /// component is rejected explicitly until its lowering is implemented.
    pub fn prepare_module(&self, module: &FirModule) -> Result<PreparedModule, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        if !module.functions.is_empty() {
            return Err(BackendError::UnsupportedFir {
                component: "functions",
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

        Ok(PreparedModule {
            target: self.target,
            context: self.new_context(),
            signature: self.new_signature(),
        })
    }
}

/// C1's prepared, still-empty Cranelift state. This becomes the owner of
/// generated CLIF functions as C2/C3 add type and CFG lowering.
pub struct PreparedModule {
    target: CraneliftTarget,
    context: Context,
    signature: Signature,
}

impl PreparedModule {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn context(&self) -> &Context {
        &self.context
    }

    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}
