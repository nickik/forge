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
            match operation {
                Sia32PrivilegedOperation::SwapScratch => {
                    return Err(BackendError::UnsupportedInstruction {
                        kind: "SIA32 scratch swap not yet represented in CLIF bridge",
                    });
                }
                Sia32PrivilegedOperation::ReturnContext => {
                    return Err(BackendError::UnsupportedInstruction {
                        kind: "SIA32 privileged operation not yet represented in CLIF bridge",
                    });
                }
                _ => {}
            }
            let register = match operation {
                Sia32PrivilegedOperation::ReadGpr { register }
                | Sia32PrivilegedOperation::WriteGpr { register } => Some(*register),
                _ => None,
            };
            if let Some(register) = register.filter(|register| *register > 15) {
                return Err(BackendError::InvalidFirShape {
                    message: format!(
                        "SIA32 GPR selector r{register} is outside the architectural r0..r15 range"
                    ),
                });
            }
            let expected = match operation {
                Sia32PrivilegedOperation::Trap { .. }
                | Sia32PrivilegedOperation::ReadSystem { .. } => 0,
                Sia32PrivilegedOperation::ReadGpr { .. } => 0,
                Sia32PrivilegedOperation::WriteSystem { .. }
                | Sia32PrivilegedOperation::WriteGpr { .. } => 1,
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
            // Immediate selectors are encoded in the FIR operation itself.
            // Only value-carrying writes retain a runtime operand.
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
    use forge_fir::{
        FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirValueId, Span,
    };
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

    #[test]
    fn rejects_gpr_selector_outside_the_architectural_register_file() {
        let mut function = empty_function();
        function.blocks[0].instructions.push(FirInstruction {
            span: Span::new(0, 1),
            result: None,
            kind: FirInstructionKind::Sia32Privileged {
                operation: Sia32PrivilegedOperation::ReadGpr { register: 16 },
                args: vec![],
            },
        });

        let error = validate_sia32_privileged_operations(CraneliftTarget::Sia32, &function)
            .expect_err("r16 must be rejected before CLIF lowering");
        assert!(matches!(
            error,
            BackendError::InvalidFirShape { message }
                if message
                    == "SIA32 GPR selector r16 is outside the architectural r0..r15 range"
        ));
    }

    #[test]
    fn rejects_privileged_operations_missing_from_the_clif_bridge() {
        let cases = [
            (
                Sia32PrivilegedOperation::SwapScratch,
                "SIA32 scratch swap not yet represented in CLIF bridge",
            ),
            (
                Sia32PrivilegedOperation::ReturnContext,
                "SIA32 privileged operation not yet represented in CLIF bridge",
            ),
        ];

        for (operation, kind) in cases {
            let mut function = empty_function();
            function.blocks[0].instructions.push(FirInstruction {
                span: Span::new(0, 1),
                result: None,
                kind: FirInstructionKind::Sia32Privileged {
                    operation,
                    args: vec![FirValueId(0)],
                },
            });

            assert_eq!(
                validate_sia32_privileged_operations(CraneliftTarget::Sia32, &function),
                Err(BackendError::UnsupportedInstruction { kind })
            );
        }
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
    ReadGpr { register: u8 },
    WriteGpr { register: u8 },
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
            Sia32PrivilegedOperation::ReadGpr { register } => Self::ReadGpr { register },
            Sia32PrivilegedOperation::WriteGpr { register } => Self::WriteGpr { register },
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
