use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget, MachineCode};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId,
    IntWidth, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn choose_module() -> (FirModule, DefId) {
    let owner = DefId(0);
    let param = FirLocalId(0);
    let ty = u64_ty();
    let span = Span::new(0, 0);
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let v2 = FirValueId(2);
    let v3 = FirValueId(3);
    let v4 = FirValueId(4);

    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(
            param,
            FirLocal {
                id: param,
                source: None,
                ty: ty.clone(),
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![
                    FirInstruction {
                        span,
                        result: Some(v0),
                        kind: FirInstructionKind::Load {
                            place: FirPlace::Local { local: param },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(v1),
                        kind: FirInstructionKind::Const {
                            value: FirConst::Integer { text: "10".into() },
                        },
                    },
                    FirInstruction {
                        span,
                        result: Some(v2),
                        kind: FirInstructionKind::Binary {
                            op: BinaryOp::Greater,
                            overflow: None,
                            left: v0,
                            right: v1,
                        },
                    },
                ],
                terminator: Some(FirTerminator::Branch {
                    condition: v2,
                    then_block: FirBlockId(1),
                    else_block: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(v3),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "1".into() },
                    },
                }],
                terminator: Some(FirTerminator::Return { value: Some(v3) }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(v4),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "2".into() },
                    },
                }],
                terminator: Some(FirTerminator::Return { value: Some(v4) }),
            },
        ],
        value_types: BTreeMap::from([
            (v0, ty.clone()),
            (v1, ty.clone()),
            (v2, Ty::Bool),
            (v3, ty.clone()),
            (v4, ty),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn scalar_roundtrip_module(owner: DefId, ty: Ty) -> (FirModule, DefId) {
    let param = FirLocalId(0);
    let scratch = FirLocalId(1);
    let span = Span::new(0, 0);
    let loaded_param = FirValueId(0);
    let loaded_scratch = FirValueId(1);

    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([
            (
                param,
                FirLocal {
                    id: param,
                    source: None,
                    ty: ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                scratch,
                FirLocal {
                    id: scratch,
                    source: None,
                    ty: ty.clone(),
                    mutable: true,
                    parameter: false,
                    synthetic: false,
                },
            ),
        ]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(loaded_param),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local: scratch },
                        value: loaded_param,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(loaded_scratch),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: scratch },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded_scratch),
            }),
        }],
        value_types: BTreeMap::from([(loaded_param, ty.clone()), (loaded_scratch, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn compile_choose() -> MachineCode {
    let backend = CraneliftBackend::riscv64().expect("RV64 backend");
    let (module, owner) = choose_module();
    let prepared = backend.prepare_module(&module).expect("verified RV64 CLIF");
    backend
        .emit_machine_code(&prepared, owner)
        .expect("RV64 machine code")
}

fn compile_scalar_roundtrip(owner: DefId, ty: Ty) -> MachineCode {
    let backend = CraneliftBackend::riscv64().expect("RV64 backend");
    let (module, owner) = scalar_roundtrip_module(owner, ty);
    let prepared = backend
        .prepare_module(&module)
        .expect("verified scalar roundtrip FIR");
    backend
        .emit_machine_code(&prepared, owner)
        .expect("RV64 scalar roundtrip machine code")
}

fn compile_duration_roundtrip() -> MachineCode {
    compile_scalar_roundtrip(DefId(1), Ty::Duration)
}

fn compile_char_roundtrip() -> MachineCode {
    compile_scalar_roundtrip(DefId(2), Ty::Char)
}

#[test]
fn emits_riscv64_machine_code_from_the_same_fir() {
    let first = compile_choose();
    let second = compile_choose();

    assert_eq!(first.target(), CraneliftTarget::Riscv64);
    assert!(!first.bytes().is_empty());
    assert_eq!(
        first.bytes().len() % 2,
        0,
        "RV64GC instruction stream alignment"
    );
    assert_eq!(
        first.bytes(),
        second.bytes(),
        "machine code must be deterministic"
    );
}

#[test]
fn executes_riscv64_machine_code_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }

    let machine = compile_choose();
    assert_eq!(run_under_qemu(&machine, 5), 2);
    assert_eq!(run_under_qemu(&machine, 10), 2);
    assert_eq!(run_under_qemu(&machine, 11), 1);
    assert_eq!(run_under_qemu(&machine, 20), 1);
}

#[test]
fn executes_duration_argument_local_and_return_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }

    let machine = compile_duration_roundtrip();
    assert_eq!(run_under_qemu(&machine, 7), 7);
    assert_eq!(run_under_qemu(&machine, 37), 37);
    assert_eq!(run_under_qemu(&machine, 229), 229);
}

#[test]
fn executes_char_argument_local_and_return_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }

    let machine = compile_char_roundtrip();
    assert_eq!(run_under_qemu(&machine, 65), 65);
    assert_eq!(run_under_qemu(&machine, 122), 122);
    assert_eq!(run_under_qemu(&machine, 195), 195);
}

#[test]
fn executes_bool_and_narrow_integer_roundtrips_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }

    let bool_machine = compile_scalar_roundtrip(DefId(3), Ty::Bool);
    assert_eq!(run_under_qemu(&bool_machine, 0), 0);
    assert_eq!(run_under_qemu(&bool_machine, 1), 1);

    for (ty, argument, expected) in [
        (
            Ty::Int {
                signed: false,
                width: IntWidth::W8,
            },
            211,
            211,
        ),
        (
            Ty::Int {
                signed: true,
                width: IntWidth::W8,
            },
            -7,
            249,
        ),
        (
            Ty::Int {
                signed: false,
                width: IntWidth::W16,
            },
            212,
            212,
        ),
        (
            Ty::Int {
                signed: true,
                width: IntWidth::W16,
            },
            -37,
            219,
        ),
        (
            Ty::Int {
                signed: false,
                width: IntWidth::W32,
            },
            213,
            213,
        ),
        (
            Ty::Int {
                signed: true,
                width: IntWidth::W32,
            },
            -91,
            165,
        ),
    ] {
        let machine = compile_scalar_roundtrip(DefId(4), ty);
        assert_eq!(run_under_qemu(&machine, argument), expected);
    }
}

fn run_under_qemu(machine: &MachineCode, argument: i64) -> i32 {
    let dir = temporary_directory(argument);
    fs::create_dir_all(&dir).expect("create RV64 test directory");
    let function = dir.join("function.bin");
    let source = dir.join("launcher.s");
    let object = dir.join("launcher.o");
    let executable = dir.join("launcher.elf");

    fs::write(&function, machine.bytes()).expect("write RV64 function bytes");
    fs::write(
        &source,
        format!(
            ".section .text\n\
             .globl _start\n\
             .type _start, @function\n\
             _start:\n\
               li a0, {argument}\n\
               call forge_fn\n\
               li a7, 93\n\
               ecall\n\
             .balign 4\n\
             .globl forge_fn\n\
             .type forge_fn, @function\n\
             forge_fn:\n\
               .incbin \"{}\"\n",
            function.display()
        ),
    )
    .expect("write RV64 launcher assembly");

    run_tool(
        "riscv64-linux-gnu-as",
        [
            "-march=rv64gc",
            "-mabi=lp64d",
            "-o",
            object.to_str().expect("object path"),
            source.to_str().expect("source path"),
        ],
    );
    run_tool(
        "riscv64-linux-gnu-ld",
        [
            "-nostdlib",
            "-static",
            "-e",
            "_start",
            "-o",
            executable.to_str().expect("executable path"),
            object.to_str().expect("object path"),
        ],
    );

    let status = Command::new("qemu-riscv64")
        .arg(&executable)
        .status()
        .expect("qemu-riscv64 must be installed when FORGE_RISCV64_EXECUTION is set");
    let _ = fs::remove_dir_all(&dir);
    status.code().expect("qemu terminated by signal")
}

fn run_tool<const N: usize>(program: &str, args: [&str; N]) {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(
        output.status.success(),
        "{program} failed with {}:\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn temporary_directory(argument: i64) -> PathBuf {
    std::env::temp_dir().join(format!("forge-rv64-{}-{argument}", std::process::id()))
}
