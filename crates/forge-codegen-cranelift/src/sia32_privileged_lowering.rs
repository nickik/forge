use forge_fir::{FirFunction, FirInstructionKind, Sia32PrivilegedOperation};

use crate::{BackendError, CraneliftTarget};

/// Validate the target-specific part of the M27 contract before generic CLIF
/// lowering.  Privileged operations have no host/AArch64/RISC-V semantics.
pub(crate) fn validate_sia32_privileged_operations(
    target: CraneliftTarget,
    function: &FirFunction,
) -> Result<(), BackendError> {
    for block in &function.blocks {
        for instruction in &block.instructions {
            let FirInstructionKind::Sia32Privileged { operation, args } = &instruction.kind else {
                continue;
            };
            if target != CraneliftTarget::Sia32 {
                return Err(BackendError::UnsupportedInstruction {
                    kind: "SIA32 privileged operation on non-SIA32 target",
                });
            }
            let expected = match operation {
                Sia32PrivilegedOperation::Trap { .. }
                | Sia32PrivilegedOperation::ReadSystem { .. } => 1,
                Sia32PrivilegedOperation::WriteSystem { .. } => 2,
                Sia32PrivilegedOperation::SwapScratch
                | Sia32PrivilegedOperation::ReturnContext
                | Sia32PrivilegedOperation::TlbFenceVa
                | Sia32PrivilegedOperation::TlbFenceAsid => 1,
                Sia32PrivilegedOperation::Return
                | Sia32PrivilegedOperation::TlbFence
                | Sia32PrivilegedOperation::WaitForInterrupt
                | Sia32PrivilegedOperation::SyncInstruction
                | Sia32PrivilegedOperation::Fence => 0,
            };
            // TRAP/SREAD carry their immediate/system-register selector both in
            // the explicit operation and as the typed source argument. Keeping
            // that argument in FIR preserves source diagnostics until the
            // dedicated SIA32 lowering consumes it.
            if args.len() != expected {
                return Err(BackendError::InvalidFirShape {
                    message: format!(
                        "SIA32 privileged operation {operation:?} has {} operands, expected {expected}",
                        args.len()
                    ),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use forge_fir::{FirBasicBlock, FirBlockId, FirFunction};
    use std::collections::BTreeMap;

    fn empty_function() -> FirFunction {
        FirFunction {
            owner: forge_fir::DefId(0),
            params: vec![],
            return_type: forge_fir::Ty::Void,
            locals: BTreeMap::new(),
            closures: BTreeMap::new(),
            entry: FirBlockId(0),
            blocks: vec![FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![],
                terminator: Some(forge_fir::FirTerminator::Return { value: None }),
            }],
            value_types: BTreeMap::new(),
        }
    }

    #[test]
    fn ordinary_function_is_target_independent() {
        let function = empty_function();
        assert!(validate_sia32_privileged_operations(CraneliftTarget::Sia32, &function).is_ok());
        assert!(validate_sia32_privileged_operations(CraneliftTarget::Aarch64, &function).is_ok());
    }
}

/// Stable bridge descriptor consumed by the SIA32 machine-backend integration.
/// Keeping this conversion in one place prevents frontend operation names from
/// leaking into Cranelift target code.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sia32MachinePrivilegedOp {
    Trap { imm8: u8 },
    ReadSystem { selector: u8 },
    WriteSystem { selector: u8 },
    SwapScratch,
    Return,
    ReturnContext,
    TlbFence,
    TlbFenceVa,
    TlbFenceAsid,
    WaitForInterrupt,
    SyncInstruction,
    Fence,
}

impl From<Sia32PrivilegedOperation> for Sia32MachinePrivilegedOp {
    fn from(value: Sia32PrivilegedOperation) -> Self {
        match value {
            Sia32PrivilegedOperation::Trap { imm8 } => Self::Trap { imm8 },
            Sia32PrivilegedOperation::ReadSystem { system_register } => Self::ReadSystem {
                selector: system_register,
            },
            Sia32PrivilegedOperation::WriteSystem { system_register } => Self::WriteSystem {
                selector: system_register,
            },
            Sia32PrivilegedOperation::SwapScratch => Self::SwapScratch,
            Sia32PrivilegedOperation::Return => Self::Return,
            Sia32PrivilegedOperation::ReturnContext => Self::ReturnContext,
            Sia32PrivilegedOperation::TlbFence => Self::TlbFence,
            Sia32PrivilegedOperation::TlbFenceVa => Self::TlbFenceVa,
            Sia32PrivilegedOperation::TlbFenceAsid => Self::TlbFenceAsid,
            Sia32PrivilegedOperation::WaitForInterrupt => Self::WaitForInterrupt,
            Sia32PrivilegedOperation::SyncInstruction => Self::SyncInstruction,
            Sia32PrivilegedOperation::Fence => Self::Fence,
        }
    }
}
