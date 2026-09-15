use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal,
    FirInstruction, FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span,
    StaticGlobalInitializer, StaticGlobalInitializerTable, StaticValue, Ty, TypeDefinitionTable,
};

const SCALAR_GLOBAL: DefId = DefId(30);
const SCALAR_READER: DefId = DefId(31);
const AGGREGATE_GLOBAL: DefId = DefId(32);
const AGGREGATE_READER: DefId = DefId(33);

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

fn aggregate_ty() -> Ty {
    Ty::Array {
        element: Box::new(u64_ty()),
        length: Some(5),
    }
}

fn scalar_reader() -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner: SCALAR_READER,
        params: Vec::new(),
        return_type: u64_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(value),
                kind: FirInstructionKind::LoadGlobal {
                    global: SCALAR_GLOBAL,
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u64_ty())]),
    }
}

fn aggregate_reader() -> FirFunction {
    let aggregate = FirValueId(0);
    let index = FirValueId(1);
    let value = FirValueId(2);
    FirFunction {
        owner: AGGREGATE_READER,
        params: Vec::new(),
        return_type: u64_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(aggregate),
                    kind: FirInstructionKind::LoadGlobal {
                        global: AGGREGATE_GLOBAL,
                    },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(index),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "4".into() },
                    },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(value),
                    kind: FirInstructionKind::IndexUnchecked {
                        base: aggregate,
                        index,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([
            (aggregate, aggregate_ty()),
            (index, usize_ty()),
            (value, u64_ty()),
        ]),
    }
}

fn fixture() -> (
    FirModule,
    TypeDefinitionTable,
    StaticGlobalInitializerTable,
) {
    let module = FirModule {
        functions: BTreeMap::from([
            (SCALAR_READER, scalar_reader()),
            (AGGREGATE_READER, aggregate_reader()),
        ]),
        globals: BTreeMap::from([
            (
                SCALAR_GLOBAL,
                FirGlobal {
                    owner: SCALAR_GLOBAL,
                    ty: u64_ty(),
                    constant: Some(ConstValue::Integer { value: 42 }),
                },
            ),
            (
                AGGREGATE_GLOBAL,
                FirGlobal {
                    owner: AGGREGATE_GLOBAL,
                    ty: aggregate_ty(),
                    constant: None,
                },
            ),
        ]),
        global_initializers: BTreeMap::new(),
        global_init_order: Vec::new(),
    };
    let integer = |value| StaticValue::Scalar(ConstValue::Integer { value });
    let static_initializers = StaticGlobalInitializerTable::from([(
        AGGREGATE_GLOBAL,
        StaticGlobalInitializer {
            value: StaticValue::Array(vec![
                integer(10),
                integer(20),
                integer(30),
                integer(40),
                integer(50),
            ]),
            writable: false,
        },
    )]);
    (module, TypeDefinitionTable::new(), static_initializers)
}

fn emit(target: CraneliftTarget) -> Vec<u8> {
    let (module, definitions, static_initializers) = fixture();
    let backend = CraneliftBackend::new(target).expect("backend");
    let prepared = backend
        .prepare_module_with_static_initializers(&module, &definitions, &static_initializers)
        .expect("C11c module should prepare");
    let object = backend
        .emit_object_with_exports(&prepared, [SCALAR_READER, AGGREGATE_READER])
        .expect("C11c object should emit");
    object.into_bytes()
}

#[test]
fn c11c_load_global_emits_real_text_relocations_on_both_targets() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let first = emit(target);
        let second = emit(target);
        assert_eq!(first, second, "{target:?} C11c object must be deterministic");
        assert_eq!(&first[..4], b"\x7fELF");

        if !tool_available("readelf") {
            continue;
        }
        let dir = temporary_directory("inspect");
        let object = dir.join("forge.o");
        fs::write(&object, first).expect("write object");
        let mut command = Command::new("readelf");
        command.args(["--wide", "-r", "-s"]).arg(&object);
        let output = successful_output(&mut command, "inspect C11c object");
        let report = String::from_utf8_lossy(&output.stdout);
        assert!(report.contains(".rela.text"), "{report}");
        assert!(
            report.contains("__forge_global_0000001e"),
            "scalar global relocation missing:\n{report}"
        );
        assert!(
            report.contains("__forge_global_00000020"),
            "aggregate global relocation missing:\n{report}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn c11c_links_and_executes_aarch64_global_reads() {
    let dir = temporary_directory("aarch64-run");
    let object = dir.join("forge.o");
    let harness = dir.join("harness.c");
    let executable = dir.join("forge-linked");
    fs::write(&object, emit(CraneliftTarget::Aarch64)).expect("write AArch64 object");
    fs::write(
        &harness,
        r#"#include <stdint.h>
extern uint64_t __forge_fn_0000001f(void);
extern uint64_t __forge_fn_00000021(void);
int main(void) {
    if (__forge_fn_0000001f() != 42) return 1;
    if (__forge_fn_00000021() != 50) return 2;
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
    successful_output(&mut linker, "link AArch64 C11c object");
    successful_output(&mut Command::new(&executable), "execute AArch64 C11c program");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn c11c_links_and_executes_riscv64_global_reads_under_qemu() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }
    for tool in [
        "riscv64-linux-gnu-as",
        "riscv64-linux-gnu-ld",
        "qemu-riscv64",
    ] {
        assert!(tool_available(tool), "required RV64 tool missing: {tool}");
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
    call __forge_fn_0000001f
    li t0, 42
    bne a0, t0, fail
    call __forge_fn_00000021
    li t0, 50
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
    .expect("write RV64 harness");

    let mut assembler = Command::new("riscv64-linux-gnu-as");
    assembler
        .args(["-march=rv64gc", "-mabi=lp64d", "-o"])
        .arg(&start)
        .arg(&source);
    successful_output(&mut assembler, "assemble RV64 C11c harness");

    let mut linker = Command::new("riscv64-linux-gnu-ld");
    linker.arg("-o").arg(&executable).arg(&start).arg(&object);
    successful_output(&mut linker, "link RV64 C11c object");

    let mut run = Command::new("qemu-riscv64");
    run.arg(&executable);
    successful_output(&mut run, "execute linked RV64 C11c program");
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
    let path = std::env::temp_dir().join(format!(
        "forge-c11c-{label}-{}-{serial}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove stale C11c temp directory");
    }
    fs::create_dir_all(&path).expect("create C11c temp directory");
    path
}
