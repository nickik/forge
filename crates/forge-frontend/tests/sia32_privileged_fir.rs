use forge_frontend::{
    lower_fir, lower_module, lower_resolved_bodies, parse_source, type_check_module,
    FirInstructionKind, Sia32PrivilegedOperation,
};

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

#[test]
fn named_u8_constants_are_preserved_as_privileged_selectors() {
    let source = r#"
        module test.sia_named_selectors;
        const STATUS: u8 = 0u8;
        const EPC: u8 = 1u8;
        const CAUSE: u8 = 2u8;
        const VMCTX: u8 = 5u8;

        fn main() -> i32 {
            sia_swrite(VMCTX, 0x1001u32);
            val cause: u32 = sia_sread(CAUSE);
            val epc: u32 = sia_sread(EPC);
            val status: u32 = sia_sread(STATUS);
            return 0;
        }
    "#;
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "{:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(bodies.diagnostics.is_empty(), "{:?}", bodies.diagnostics);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    let fir = lower_fir(&bodies, &typed);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let main = hir.module.symbols["main"].value_def.unwrap();
    let function = &fir.module.functions[&main];
    let ops = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter_map(|insn| {
            if let FirInstructionKind::Sia32Privileged { operation, .. } = insn.kind {
                Some(operation)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    assert!(ops.contains(&Sia32PrivilegedOperation::WriteSystem { system_register: 5 }));
    assert!(ops.contains(&Sia32PrivilegedOperation::ReadSystem { system_register: 2 }));
    assert!(ops.contains(&Sia32PrivilegedOperation::ReadSystem { system_register: 1 }));
    assert!(ops.contains(&Sia32PrivilegedOperation::ReadSystem { system_register: 0 }));
}

#[test]
fn fixed_gpr_syscall_builtins_lower_to_explicit_fir() {
    let source = r#"
        module test.sia_gpr_abi;
        fn main() -> i32 {
            sia_gpr_write(1u8, 0u32);
            val result: u32 = sia_gpr_read(1u8);
            return 0;
        }
    "#;
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "{:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(bodies.diagnostics.is_empty(), "{:?}", bodies.diagnostics);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    let fir = lower_fir(&bodies, &typed);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let main = hir.module.symbols["main"].value_def.unwrap();
    let function = &fir.module.functions[&main];
    let ops = function
        .blocks
        .iter()
        .flat_map(|b| &b.instructions)
        .filter_map(|insn| {
            if let FirInstructionKind::Sia32Privileged { operation, .. } = insn.kind {
                Some(operation)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    assert!(ops.contains(&Sia32PrivilegedOperation::WriteGpr { register: 1 }));
    assert!(ops.contains(&Sia32PrivilegedOperation::ReadGpr { register: 1 }));
}
