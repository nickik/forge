use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

const AGGREGATE_IDENTITY: DefId = DefId(30);
const INDIRECT_AGGREGATE_ENTRY: DefId = DefId(31);

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
                    place: FirPlace::Local { local: param },
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty)]),
    }
}

fn indirect_aggregate_entry() -> FirFunction {
    let span = Span::new(0, 0);
    let aggregate_ty = five_words_ty();
    let callee_ty = function_ty(vec![aggregate_ty.clone()], aggregate_ty.clone());
    let callee = FirValueId(0);
    let items = [
        FirValueId(1),
        FirValueId(2),
        FirValueId(3),
        FirValueId(4),
        FirValueId(5),
    ];
    let aggregate = FirValueId(6);
    let returned = FirValueId(7);
    let index = FirValueId(8);
    let extracted = FirValueId(9);

    let mut instructions = vec![FirInstruction {
        span,
        result: Some(callee),
        kind: FirInstructionKind::FunctionRef {
            target: AGGREGATE_IDENTITY,
        },
    }];
    for (value, text) in items.into_iter().zip(["10", "20", "30", "40", "50"]) {
        instructions.push(constant(span, value, text));
    }
    instructions.extend([
        FirInstruction {
            span,
            result: Some(aggregate),
            kind: FirInstructionKind::MakeArray {
                items: items.to_vec(),
            },
        },
        FirInstruction {
            span,
            result: Some(returned),
            kind: FirInstructionKind::CallIndirect {
                callee,
                args: vec![aggregate],
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

    let mut value_types = BTreeMap::from([(callee, callee_ty)]);
    for value in items {
        value_types.insert(value, u64_ty());
    }
    value_types.insert(aggregate, aggregate_ty.clone());
    value_types.insert(returned, aggregate_ty);
    value_types.insert(index, usize_ty());
    value_types.insert(extracted, u64_ty());

    FirFunction {
        owner: INDIRECT_AGGREGATE_ENTRY,
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

fn emit(target: CraneliftTarget) -> Vec<u8> {
    let mut module = FirModule::default();
    module
        .functions
        .insert(AGGREGATE_IDENTITY, aggregate_identity());
    module
        .functions
        .insert(INDIRECT_AGGREGATE_ENTRY, indirect_aggregate_entry());

    let backend = CraneliftBackend::new(target).expect("backend");
    let prepared = backend
        .prepare_module_with_types(&module, &TypeDefinitionTable::new())
        .expect("indirect aggregate fixture lowers through C9");
    backend
        .emit_object_with_exports(&prepared, [INDIRECT_AGGREGATE_ENTRY])
        .expect("indirect aggregate object emission")
        .into_bytes()
}

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn indirect_aggregate_call_links_and_executes_on_aarch64() {
    let dir = temporary_directory("indirect-aggregate-aarch64");
    let object = dir.join("forge.o");
    let harness = dir.join("harness.c");
    let executable = dir.join("forge-linked");
    fs::write(&object, emit(CraneliftTarget::Aarch64)).expect("write AArch64 object");
    fs::write(
        &harness,
        r#"#include <stdint.h>
extern uint64_t __forge_fn_0000001f(void);
int main(void) {
    return __forge_fn_0000001f() == 50 ? 0 : 1;
}
"#,
    )
    .expect("write AArch64 harness");

    let mut linker = Command::new("cc");
    linker
        .args(["-O0", "-no-pie"])
        .arg(&harness)
        .arg(&object)
        .arg("-o")
        .arg(&executable);
    successful_output(&mut linker, "link AArch64 indirect aggregate object");

    let mut run = Command::new(&executable);
    successful_output(&mut run, "execute AArch64 indirect aggregate object");
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn indirect_aggregate_call_links_and_executes_on_riscv64() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }
    for tool in [
        "riscv64-linux-gnu-as",
        "riscv64-linux-gnu-ld",
        "qemu-riscv64",
    ] {
        assert!(tool_available(tool), "required RV64 execution tool missing: {tool}");
    }

    let dir = temporary_directory("indirect-aggregate-riscv64");
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
    successful_output(&mut assembler, "assemble RV64 indirect aggregate harness");

    let mut linker = Command::new("riscv64-linux-gnu-ld");
    linker.arg("-o").arg(&executable).arg(&start).arg(&object);
    successful_output(&mut linker, "link RV64 indirect aggregate object");

    let mut run = Command::new("qemu-riscv64");
    run.arg(&executable);
    successful_output(&mut run, "execute RV64 indirect aggregate object");
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
