use cranelift_codegen::ir::{self, types};
use forge_fir::{IntWidth, Ty};

use crate::BackendError;

/// Target ABI/layout facts owned by Forge rather than inferred from convenient
/// Cranelift encodings. Aggregate layout will be added here in later milestones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetLayout {
    pub pointer_bits: u16,
}

impl TargetLayout {
    pub const fn new(pointer_bits: u16) -> Self {
        Self { pointer_bits }
    }

    pub fn pointer_type(&self) -> Result<ir::Type, BackendError> {
        match self.pointer_bits {
            32 => Ok(types::I32),
            64 => Ok(types::I64),
            pointer_bits => Err(BackendError::UnsupportedTargetLayout { pointer_bits }),
        }
    }
}

/// Mechanical FIR value-representation lowering.
///
/// This layer does not choose Forge semantics. It maps already-resolved FIR
/// types onto CLIF scalar value types according to the Forge target layout.
pub struct TypeLowering<'a> {
    target: &'a TargetLayout,
}

impl<'a> TypeLowering<'a> {
    pub const fn new(target: &'a TargetLayout) -> Self {
        Self { target }
    }

    pub const fn target(&self) -> &'a TargetLayout {
        self.target
    }

    pub fn value_type(&self, ty: &Ty) -> Result<ir::Type, BackendError> {
        match ty {
            // Forge bool has a canonical one-byte value/storage representation.
            // Comparison results must be normalized to that representation at
            // FIR/CLIF operation boundaries rather than changing the ABI here.
            Ty::Bool | Ty::Byte => Ok(types::I8),

            Ty::Int { width, .. } => match width {
                IntWidth::W8 => Ok(types::I8),
                IntWidth::W16 => Ok(types::I16),
                IntWidth::W32 => Ok(types::I32),
                IntWidth::W64 => Ok(types::I64),
                IntWidth::Pointer => self.target.pointer_type(),
            },

            // The frontend has already decided whether these operations/types
            // are legal. Codegen only supplies their machine representation.
            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {
                self.target.pointer_type()
            }

            // These are valid Forge/FIR concepts but C2 intentionally does not
            // establish their ABI or aggregate representation accidentally.
            Ty::Never => unsupported("never"),
            Ty::Void => unsupported("void"),
            Ty::Char => unsupported("char"),
            Ty::Str => unsupported("str"),
            Ty::Float { .. } => unsupported("float"),
            Ty::Duration => unsupported("duration"),
            Ty::ContextSlot { .. } => unsupported("context slot"),
            Ty::Nominal(_) => unsupported("nominal/aggregate"),
            Ty::Optional { .. } => unsupported("optional"),
            Ty::Slice { .. } => unsupported("slice"),
            Ty::Array { .. } => unsupported("array"),
            Ty::Result { .. } => unsupported("result"),
            Ty::Closure { .. } => unsupported("closure"),

            // These are frontend inference/literal sentinels. Reaching backend
            // lowering is an invariant violation, not an unsupported feature.
            Ty::Error => semantic_leak("error"),
            Ty::Unknown => semantic_leak("unknown"),
            Ty::IntLiteral => semantic_leak("integer literal"),
            Ty::FloatLiteral => semantic_leak("float literal"),
            Ty::NoneLiteral => semantic_leak("none literal"),
        }
    }
}

fn unsupported(kind: &'static str) -> Result<ir::Type, BackendError> {
    Err(BackendError::UnsupportedType { kind })
}

fn semantic_leak(kind: &'static str) -> Result<ir::Type, BackendError> {
    Err(BackendError::SemanticTypeLeak { kind })
}
