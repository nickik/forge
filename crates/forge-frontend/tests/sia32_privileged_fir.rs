use forge_frontend::{FirInstructionKind, Sia32PrivilegedOperation};

#[test]
fn sia32_privileged_fir_is_explicit_and_target_owned() {
    let operation = Sia32PrivilegedOperation::WriteSystem { system_register: 5 };
    let instruction = FirInstructionKind::Sia32Privileged {
        operation,
        args: Vec::new(),
    };
    assert!(matches!(
        instruction,
        FirInstructionKind::Sia32Privileged {
            operation: Sia32PrivilegedOperation::WriteSystem { system_register: 5 },
            ..
        }
    ));
}

#[test]
fn sia32_privileged_fir_covers_cosmic_m27_operations() {
    let required = [
        Sia32PrivilegedOperation::Trap { imm8: 1 },
        Sia32PrivilegedOperation::ReadSystem { system_register: 2 },
        Sia32PrivilegedOperation::WriteSystem { system_register: 5 },
        Sia32PrivilegedOperation::Return,
        Sia32PrivilegedOperation::TlbFence,
        Sia32PrivilegedOperation::WaitForInterrupt,
    ];
    assert_eq!(required.len(), 6);
}
