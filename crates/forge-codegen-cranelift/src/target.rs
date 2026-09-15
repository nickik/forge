use std::str::FromStr;

use cranelift_codegen::isa::{self, OwnedTargetIsa};
use cranelift_codegen::settings;
use target_lexicon::Triple;

use crate::BackendError;

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

    pub const fn layout(self) -> TargetLayout {
        match self {
            Self::Aarch64 | Self::Riscv64 => TargetLayout::new(64),
        }
    }

    pub(crate) fn isa(self) -> Result<OwnedTargetIsa, BackendError> {
        let triple = Triple::from_str(self.triple()).map_err(|error| BackendError::InvalidTarget {
            triple: self.triple(),
            message: error.to_string(),
        })?;
        let flags = settings::Flags::new(settings::builder());
        let builder = isa::lookup(triple).map_err(|error| BackendError::InvalidTarget {
            triple: self.triple(),
            message: error.to_string(),
        })?;
        builder.finish(flags).map_err(|error| BackendError::Cranelift {
            message: error.to_string(),
        })
    }
}

/// Forge-owned target ABI/layout facts. Cranelift value types are derived from
/// this description; they do not define Forge layout policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetLayout {
    pub pointer_bits: u16,
}

impl TargetLayout {
    pub const fn new(pointer_bits: u16) -> Self {
        Self { pointer_bits }
    }
}
