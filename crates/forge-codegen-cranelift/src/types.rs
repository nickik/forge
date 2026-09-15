use cranelift_codegen::ir::{self, types};
use forge_fir::{IntWidth, Ty};

use crate::{BackendError, TargetLayout};

/// Forge-owned size/alignment for scalar values that can live in memory.
///
/// This is deliberately separate from CLIF `Type`: aggregate/enum layout will
/// extend the Forge ABI/layout layer later rather than treating Cranelift's
/// value representation as Forge's layout specification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScalarLayout {
    pub size_bytes: u32,
    pub align_bytes: u32,
}

impl ScalarLayout {
    const fn new(size_bytes: u32, align_bytes: u32) -> Self {
        Self {
            size_bytes,
            align_bytes,
        }
    }

    pub fn align_shift(self) -> u8 {
        debug_assert!(self.align_bytes.is_power_of_two());
        self.align_bytes.trailing_zeros() as u8
    }
}

/// Mechanical FIR value-representation lowering.
///
/// This layer does not choose Forge semantics. It maps already-resolved FIR
/// types onto Forge scalar layout facts and CLIF scalar value types according
/// to the Forge target layout.
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

    pub fn pointer_type(&self) -> Result<ir::Type, BackendError> {
        match self.target.pointer_bits {
            32 => Ok(types::I32),
            64 => Ok(types::I64),
            pointer_bits => Err(BackendError::UnsupportedTargetLayout { pointer_bits }),
        }
    }

    pub fn scalar_layout(&self, ty: &Ty) -> Result<ScalarLayout, BackendError> {
        match ty {
            Ty::Bool | Ty::Byte => Ok(ScalarLayout::new(1, 1)),
            Ty::Int { width, .. } => Ok(match width {
                IntWidth::W8 => ScalarLayout::new(1, 1),
                IntWidth::W16 => ScalarLayout::new(2, 2),
                IntWidth::W32 => ScalarLayout::new(4, 4),
                IntWidth::W64 => ScalarLayout::new(8, 8),
                IntWidth::Pointer => self.pointer_layout()?,
            }),
            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {
                self.pointer_layout()
            }

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

            Ty::Error => semantic_leak("error"),
            Ty::Unknown => semantic_leak("unknown"),
            Ty::IntLiteral => semantic_leak("integer literal"),
            Ty::FloatLiteral => semantic_leak("float literal"),
            Ty::NoneLiteral => semantic_leak("none literal"),
        }
    }

    pub fn value_type(&self, ty: &Ty) -> Result<ir::Type, BackendError> {
        match ty {
            // Forge bool has a canonical one-byte value/storage representation.
            Ty::Bool | Ty::Byte => Ok(types::I8),

            Ty::Int { width, .. } => match width {
                IntWidth::W8 => Ok(types::I8),
                IntWidth::W16 => Ok(types::I16),
                IntWidth::W32 => Ok(types::I32),
                IntWidth::W64 => Ok(types::I64),
                IntWidth::Pointer => self.pointer_type(),
            },

            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {
                self.pointer_type()
            }

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

            Ty::Error => semantic_leak("error"),
            Ty::Unknown => semantic_leak("unknown"),
            Ty::IntLiteral => semantic_leak("integer literal"),
            Ty::FloatLiteral => semantic_leak("float literal"),
            Ty::NoneLiteral => semantic_leak("none literal"),
        }
    }

    fn pointer_layout(&self) -> Result<ScalarLayout, BackendError> {
        match self.target.pointer_bits {
            32 => Ok(ScalarLayout::new(4, 4)),
            64 => Ok(ScalarLayout::new(8, 8)),
            pointer_bits => Err(BackendError::UnsupportedTargetLayout { pointer_bits }),
        }
    }
}

fn unsupported<T>(kind: &'static str) -> Result<T, BackendError> {
    Err(BackendError::UnsupportedType { kind })
}

fn semantic_leak<T>(kind: &'static str) -> Result<T, BackendError> {
    Err(BackendError::SemanticTypeLeak { kind })
}
