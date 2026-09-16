use cranelift_codegen::isa::{self, OwnedTargetIsa};
use cranelift_codegen::settings;

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
        // Let Cranelift's own SIA-enabled target-lexicon type be inferred here.
        // Forge deliberately does not depend on a second target-lexicon crate.
        let triple = self
            .triple()
            .parse()
            .map_err(|error: cranelift_codegen::isa::LookupError| BackendError::InvalidTarget {
                triple: self.triple(),
                message: error.to_string(),
            });
        let triple = match triple {
            Ok(triple) => triple,
            Err(_) => {
                // Parsing and ISA lookup use different error types, so parse in
                // the lookup call below where the expected Triple is known.
                return self.isa_from_inferred_triple();
            }
        };
        let flags = settings::Flags::new(settings::builder());
        let builder = isa::lookup(triple).map_err(|error| BackendError::InvalidTarget {
            triple: self.triple(),
            message: error.to_string(),
        })?;
        builder.finish(flags).map_err(|error| BackendError::Cranelift {
            message: error.to_string(),
        })
    }

    fn isa_from_inferred_triple(self) -> Result<OwnedTargetIsa, BackendError> {
        let triple = self.triple().parse().map_err(|error| BackendError::InvalidTarget {
            triple: self.triple(),
            message: format!("{error:?}"),
        })?;
        let flags = settings::Flags::new(settings::builder());
        isa::lookup(triple)
            .map_err(|error| BackendError::InvalidTarget {
                triple: self.triple(),
                message: error.to_string(),
            })?
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
