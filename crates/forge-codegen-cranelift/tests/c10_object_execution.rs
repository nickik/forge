use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget};
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirTerminator, FirValueId, IntWidth,
    OverflowMode, Span, Ty, TypeDefinitionTable,
};

const AGGREGATE_IDENTITY: DefId = DefId(10);
const AGGREGATE_ENTRY: DefId = DefId(11);
const SCALAR_ADD_ONE: DefId = DefId(20);
const FUNCTION_REF_ENTRY: DefId = DefId(21);

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn usize_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    }
}

fn five_words_ty() -> Ty {
    Ty::Array {
        element: Box::new(u64_ty()),
        length: Some(5),
    }
}

fn function_ty(params: Vec<Ty>, result: Ty) -> Ty {
    Ty::Function {
        params,
        result: Box::new(result),
        named_arguments: false,
    }
}

fn parameter(id: u32, ty: Ty) -> (FirLocalId, FirLocal) {
    let id = FirLocalId(id);
    (
        id,
        FirLocal {
            id,
            source: None,
            ty,
            mutable: false,
            parameter: true,
            synthetic: false,
        },
    )
}

fn constant(span: Span, result: FirValueId, text: &str) -> FirInstruction {
    FirInstruction {
        span,
        result: Some(result),
        kind: FirInstructionKind::Const {
            value: FirConst::Integer { text: text.into() },
        },
    }
}

fn aggregate_identity() -> FirFunction {
    let span = Span::new(0, 0);
    let ty = five_words_ty();
    let (param, local) = parameter(0, ty.clone());
    let loaded = FirValueId(0);
    FirFunction {
        owner: AGGREGATE_IDENTITY,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, local)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(loaded),
                kind: FirInstructionKind::Load {
                    place: forge_fir::FirPlace::Local { local: param },
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty)]),
    }
}

fn aggregate_entry() -> FirFunction {
    let span = Span::new(0, 0);
    let array_ty = five_words_ty();
    let values = [
        FirValueId(0),
        FirValueId(1),
        FirValueId(2),
        FirValueId(3),
        FirValueId(4),
    ];
    let made = FirValueId(5);
    let returned = FirValueId(6);
    let index = FirValueId(7);
    let extracted = FirValueId(8);

    let mut instructions = values
        .iter()
        .enumerate()
        .map(|(index, value)| constant(span, *value, &(index + 1).to_string()))
        .collect::<Vec<_>>();
    instructions.extend([
        FirInstruction {
            span,
            result: Some(made),
            kind: FirInstructionKind::MakeArray {
                items: values.to_vec(),
            },
        },
        FirInstruction {
            span,
            result: Some(returned),
            kind: FirInstructionKind::Call {
                target: AGGREGATE_IDENTITY,
                args: vec![made],
                tail: false,
            },
        },
        constant(span, index, "4"),
        FirInstruction {
            span,
            result: Some(extracted),
            kind: FirInstructionKind::IndexUnchecked {
                base: returned,
                index,
            },
        },
    ]);

    let mut value_types = BTreeMap::new();
    for value in values {
        value_types.insert(value, u64_ty());
    }
    value_types.insert(made, array_ty.clone());
    value_types.insert(returned, array_ty);
    value_types.insert(index, usize_ty());
    value_types.insert(extracted, u64_ty());

    FirFunction {
        owner: AGGREGATE_ENTRY,
        params: vec![],
        return_type: u64_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions,
            terminator: Some(FirTerminator::Return {
                value: Some(extracted),
            }),
        }],
        value_types,
    }
}

fn scalar_add_one() -> FirFunction {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let (param, local) = parameter(0, ty.clone());
    let loaded = FirValueId(0);
    let one = FirValueId(1);
    let result = FirValueId(2);
    FirFunction {
        owner: SCALAR_ADD_ONE,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(param, local)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: forge_fir::FirPlace::Local { local: param },
                    },
                },
                constant(span, one, "1"),
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Binary {
                        op: BinaryOp::Add,
                        overflow: Some(OverflowMode::Wrapping),
                        left: loaded,
                        right: one,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty.clone()), (one, ty.clone()), (result, ty)]),
    }
}

fn function_ref_entry() -> FirFunction {
    let span = Span::new(0, 0);
    let fn_ty = function_ty(vec![u64_ty()], u64_ty());
    let function = FirValueId(0);
    let argument = FirValueId(1);
    let result = FirValueId(2);
    FirFunction {
        owner: FUNCTION_REF_ENTRY,
        params: vec![],
        return_type: u64_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(function),
                    kind: FirInstructionKind::FunctionRef {
                        target: SCALAR_ADD_ONE,
                    },
                },
                constant(span, argument, "41"),
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::CallIndirect {
                        callee: function,
                        args: vec![argument],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(function, fn_ty), (argument, u64_ty()), (result, u64_ty())]),
    }
}

fn fixture() -> (FirModule, TypeDefinitionTable) {
    let mut module = FirModule::default();
    module
        .functions
        .insert(AGGREGATE_IDENTITY, aggregate_identity());
    module.functions.insert(AGGREGATE_ENTRY, aggregate_entry());
    module.functions.insert(SCALAR_ADD_ONE, scalar_add_one());
    module
        .functions
        .insert(FUNCTION_REF_ENTRY, function_ref_entry());
    (module, TypeDefinitionTable::new())
}

fn emit(target: CraneliftTarget) -> Vec<u8> {
    let (module, definitions) = fixture();
    let backend = CraneliftBackend::new(target).expect("backend");
    let prepared = backend
        .prepare_module_with_types(&module, &definitions)
        .expect("C10 fixture should lower through C9");
    let object = backend
        .emit_object_with_exports(&prepared, [AGGREGATE_ENTRY, FUNCTION_REF_ENTRY])
        .expect("C10 object should emit");
    assert_eq!(object.target(), target);
    assert_eq!(object.plan().symbols().len(), 4);
    object.into_bytes()
}

#[test]
fn c10b_c_object_is_deterministic_and_contains_real_relocations() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let first = emit(target);
        let second = emit(target);
        assert_eq!(
            first, second,
            "{target:?} object emission must be deterministic"
        );
        assert_eq!(&first[..4], b"\x7fELF");
        assert_eq!(first[4], 2, "ELF64");
        assert_eq!(first[5], 1, "little endian");

        if !tool_available("readelf") {
            continue;
        }
        let dir = temporary_directory("inspect");
        let object = dir.join("forge.o");
        fs::write(&object, &first).expect("write object");
        let mut command = Command::new("readelf");
        command.args(["--wide", "-h", "-s", "-r"]).arg(&object);
        let output = successful_output(&mut command, "readelf emitted object");
        let report = String::from_utf8_lossy(&output.stdout);
        match target {
            CraneliftTarget::Aarch64 => assert!(report.contains("AArch64"), "{report}"),
            CraneliftTarget::Riscv64 => assert!(report.contains("RISC-V"), "{report}"),
        }
        for symbol in [
            "__forge_fn_0000000a",
            "__forge_fn_0000000b",
            "__forge_fn_00000014",
            "__forge_fn_00000015",
        ] {
            assert!(report.contains(symbol), "missing {symbol}:\n{report}");
        }
        assert!(
            report.contains("__forge_fn_0000000a"),
            "direct aggregate-call relocation missing:\n{report}"
        );
        assert!(
            report.contains("__forge_fn_00000014"),
            "function-address relocation missing:\n{report}"
        );
        assert!(report.contains(".rela.text"), "{report}");
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn c10d_links_and_executes_aarch64_object() {
    let dir = temporary_directory("aarch64-run");
    let object = dir.join("forge.o");
    let harness = dir.join("harness.c");
    let executable = dir.join("forge-linked");
    fs::write(&object, emit(CraneliftTarget::Aarch64)).expect("write AArch64 object");
    fs::write(
        &harness,
        r#"#include <stdint.h>
extern uint64_t __forge_fn_0000000b(void);
extern uint64_t __forge_fn_00000015(void);
int main(void) {
    if (__forge_fn_0000000b() != 5) return 11;
    if (__forge_fn_00000015() != 42) return 21;
    return 0;
}
"#,
    )
    .expect("write C harness");

    let mut linker = Command::new("cc");
    linker
        .args(["-O0", "-no-pie"])
        .arg(&harness)
        .arg(&object)
        .arg("-o")
        .arg(&executable);
    successful_output(&mut linker, "link AArch64 C10 object");

    let mut run = Command::new(&executable);
    successful_output(&mut run, "execute linked AArch64 C10 program");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn c10d_links_and_executes_riscv64_object_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }
    for tool in [
        "riscv64-linux-gnu-as",
        "riscv64-linux-gnu-ld",
        "qemu-riscv64",
    ] {
        assert!(
            tool_available(tool),
            "required RV64 execution tool missing: {tool}"
        );
    }

    let dir = temporary_directory("riscv64-run");
    let object = dir.join("forge.o");
    let source = dir.join("start.S");
    let start = dir.join("start.o");
    let executable = dir.join("forge-linked");
    fs::write(&object, emit(CraneliftTarget::Riscv64)).expect("write RV64 object");
    fs::write(
        &source,
        r#".section .text
.globl _start
_start:
    call __forge_fn_0000000b
    li t0, 5
    bne a0, t0, fail
    call __forge_fn_00000015
    li t0, 42
    bne a0, t0, fail
    li a0, 0
    li a7, 93
    ecall
fail:
    li a0, 1
    li a7, 93
    ecall
"#,
    )
    .expect("write RV64 start assembly");

    let mut assembler = Command::new("riscv64-linux-gnu-as");
    assembler
        .args(["-march=rv64gc", "-mabi=lp64d", "-o"])
        .arg(&start)
        .arg(&source);
    successful_output(&mut assembler, "assemble RV64 C10 harness");

    let mut linker = Command::new("riscv64-linux-gnu-ld");
    linker.arg("-o").arg(&executable).arg(&start).arg(&object);
    successful_output(&mut linker, "link RV64 C10 object");

    let mut run = Command::new("qemu-riscv64");
    run.arg(&executable);
    successful_output(&mut run, "execute linked RV64 C10 program");
    let _ = fs::remove_dir_all(dir);
}

fn tool_available(program: &str) -> bool {
    Command::new(program)
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

fn successful_output(command: &mut Command, label: &str) -> Output {
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("{label}: failed to start: {error}"));
    assert!(
        output.status.success(),
        "{label}: status={}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn temporary_directory(label: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("forge-c10-{label}-{}-{serial}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove stale C10 temp directory");
    }
    fs::create_dir_all(&path).expect("create C10 temp directory");
    path
}
