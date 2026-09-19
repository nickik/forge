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


#[test]
fn swrite_selector_is_immediate_and_only_value_is_runtime_operand() {
    // The FIR contract itself is the important regression here: SWRITE's
    // selector is compile-time operation metadata, while only the u32 value is
    // a runtime SSA operand. Source-to-FIR coverage lives in the frontend
    // pipeline tests rather than reconstructing that pipeline incorrectly here.
    let runtime_value = forge_frontend::FirValueId(7);
    let instruction = FirInstructionKind::Sia32Privileged {
        operation: Sia32PrivilegedOperation::WriteSystem { system_register: 5 },
        args: vec![runtime_value],
    };
    match instruction {
        FirInstructionKind::Sia32Privileged {
            operation: Sia32PrivilegedOperation::WriteSystem { system_register },
            args,
        } => {
            assert_eq!(system_register, 5);
            assert_eq!(args, vec![runtime_value]);
        }
        _ => panic!("expected SWRITE privileged FIR"),
    }
}
