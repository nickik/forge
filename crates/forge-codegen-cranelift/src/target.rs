use std::str::FromStr;

use cranelift_codegen::isa::{self, OwnedTargetIsa};
use cranelift_codegen::settings;
use target_lexicon::Triple;

use crate::BackendError;

/// Forge code-generation targets. SIA32 is selectable before general CLIF
/// lowering is complete so target configuration and the M8 object/image path
/// can be exercised independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CraneliftTarget {
    Aarch64,
    Riscv64,
    Sia32,
}

impl CraneliftTarget {
    pub const fn triple(self) -> &'static str {
        match self {
            Self::Aarch64 => "aarch64-unknown-linux-gnu",
            Self::Riscv64 => "riscv64gc-unknown-linux-gnu",
            Self::Sia32 => "sia32-unknown-none",
        }
    }

    pub const fn layout(self) -> TargetLayout {
        match self {
            Self::Aarch64 | Self::Riscv64 => TargetLayout::new(64),
            Self::Sia32 => TargetLayout::new(32),
        }
    }

    pub const fn abi(self) -> TargetAbi {
        match self {
            Self::Aarch64 | Self::Riscv64 => TargetAbi::SystemV,
            Self::Sia32 => TargetAbi::Sia32,
        }
    }

    pub const fn executable_format(self) -> ExecutableFormat {
        match self {
            Self::Aarch64 | Self::Riscv64 => ExecutableFormat::Elf64,
            Self::Sia32 => ExecutableFormat::Sia32FlatImage,
        }
    }

    pub(crate) fn isa(self) -> Result<OwnedTargetIsa, BackendError> {
        let triple =
            Triple::from_str(self.triple()).map_err(|error| BackendError::InvalidTarget {
                triple: self.triple(),
                message: error.to_string(),
            })?;
        let flags = settings::Flags::new(settings::builder());
        let builder = isa::lookup(triple).map_err(|error| BackendError::InvalidTarget {
            triple: self.triple(),
            message: error.to_string(),
        })?;
        builder
            .finish(flags)
            .map_err(|error| BackendError::Cranelift {
                message: error.to_string(),
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetAbi {
    SystemV,
    Sia32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutableFormat {
    Elf64,
    Sia32FlatImage,
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
