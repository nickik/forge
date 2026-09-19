use forge_frontend::{lower_module, lower_resolved_bodies, parse_source, FirInstructionKind, Sia32PrivilegedOperation};

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
    let source = r#"
        module test.sia;
        pub fn write_vmctx(value: u32) -> void {
            sia_swrite(5u8, value);
        }
    "#;
    let parsed = parse_source(source).expect("parse");
    let lowered = lower_module(&parsed).expect("lower module");
    let fir = lower_resolved_bodies(&lowered).expect("lower FIR");
    let function = fir.functions.values().find(|f| f.name == "write_vmctx").expect("function");
    let privileged = function.blocks.iter().flat_map(|b| b.instructions.iter()).find_map(|inst| {
        match &inst.kind {
            FirInstructionKind::Sia32Privileged { operation: Sia32PrivilegedOperation::WriteSystem { system_register }, args } =>
                Some((*system_register, args.clone())),
            _ => None,
        }
    }).expect("SWRITE FIR");
    assert_eq!(privileged.0, 5);
    assert_eq!(privileged.1.len(), 1, "selector must not survive as a runtime FIR operand");
}
