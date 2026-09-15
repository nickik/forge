use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget, ObjectLinkage};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal, FirGlobalInitializer,
    FirInstruction, FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

const FIRST: DefId = DefId(10);
const SECOND: DefId = DefId(11);
const PAIR: DefId = DefId(12);

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn pair_ty() -> Ty {
    Ty::Array {
        element: Box::new(u64_ty()),
        length: Some(2),
    }
}

fn return_u64(owner: DefId, text: &str) -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner,
        params: vec![],
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
                kind: FirInstructionKind::Const {
                    value: FirConst::Integer { text: text.into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u64_ty())]),
    }
}

fn copy_global(owner: DefId, global: DefId) -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner,
        params: vec![],
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
                kind: FirInstructionKind::LoadGlobal { global },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u64_ty())]),
    }
}

fn make_pair(owner: DefId) -> FirFunction {
    let first = FirValueId(0);
    let second = FirValueId(1);
    let pair = FirValueId(2);
    FirFunction {
        owner,
        params: vec![],
        return_type: pair_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(first),
                    kind: FirInstructionKind::LoadGlobal { global: FIRST },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(second),
                    kind: FirInstructionKind::LoadGlobal { global: SECOND },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(pair),
                    kind: FirInstructionKind::MakeArray {
                        items: vec![first, second],
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(pair) }),
        }],
        value_types: BTreeMap::from([
            (first, u64_ty()),
            (second, u64_ty()),
            (pair, pair_ty()),
        ]),
    }
}

fn runtime_module() -> FirModule {
    FirModule {
        functions: BTreeMap::new(),
        globals: BTreeMap::from([
            (
                FIRST,
                FirGlobal {
                    owner: FIRST,
                    ty: u64_ty(),
                    constant: None,
                },
            ),
            (
                SECOND,
                FirGlobal {
                    owner: SECOND,
                    ty: u64_ty(),
                    constant: None,
                },
            ),
            (
                PAIR,
                FirGlobal {
                    owner: PAIR,
                    ty: pair_ty(),
                    constant: None,
                },
            ),
        ]),
        global_initializers: BTreeMap::from([
            (
                FIRST,
                FirGlobalInitializer {
                    owner: FIRST,
                    dependencies: vec![],
                    function: return_u64(FIRST, "40"),
                },
            ),
            (
                SECOND,
                FirGlobalInitializer {
                    owner: SECOND,
                    dependencies: vec![FIRST],
                    function: copy_global(SECOND, FIRST),
                },
            ),
            (
                PAIR,
                FirGlobalInitializer {
                    owner: PAIR,
                    dependencies: vec![FIRST, SECOND],
                    function: make_pair(PAIR),
                },
            ),
        ]),
        global_init_order: vec![FIRST, SECOND, PAIR],
    }
}

struct EmittedFixture {
    bytes: Vec<u8>,
    module_initializer: String,
    globals: BTreeMap<DefId, String>,
    initializer_symbols: Vec<String>,
}

fn emit(target: CraneliftTarget) -> EmittedFixture {
    let backend = CraneliftBackend::new(target).expect("backend");
    let prepared = backend.prepare_module(&runtime_module()).expect("prepare C11d module");
    assert_eq!(prepared.global_init_order(), &[FIRST, SECOND, PAIR]);
    assert_eq!(prepared.runtime_initializer_functions().len(), 3);
    assert_eq!(prepared.functions().len(), 4);

    let module_initializer = prepared
        .module_initializer_owner()
        .expect("module initializer owner");
    let plan = backend
        .plan_object_module_with_exports(&prepared, [module_initializer, FIRST, SECOND, PAIR])
        .expect("C11d object plan");
    assert_eq!(
        plan.symbol(module_initializer)
            .expect("module initializer symbol")
            .linkage(),
        ObjectLinkage::Export
    );

    let initializer_symbols = prepared
        .runtime_initializer_functions()
        .values()
        .map(|owner| {
            let symbol = plan.symbol(*owner).expect("runtime initializer symbol");
            assert_eq!(symbol.linkage(), ObjectLinkage::Local);
            symbol.name().to_owned()
        })
        .collect::<Vec<_>>();
    let globals = [FIRST, SECOND, PAIR]
        .into_iter()
        .map(|owner| {
            (
                owner,
                plan.global_symbol(owner)
                    .expect("runtime global symbol")
                    .name()
                    .to_owned(),
            )
        })
        .collect();
    let module_initializer_name = plan
        .symbol(module_initializer)
        .expect("module initializer symbol")
        .name()
        .to_owned();
    let object = backend.emit_object(&prepared, &plan).expect("emit C11d object");
    EmittedFixture {
        bytes: object.into_bytes(),
        module_initializer: module_initializer_name,
        globals,
        initializer_symbols,
    }
}

#[test]
fn c11d_compiles_one_ordered_module_initializer_on_both_targets() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let fixture = emit(target);
        assert_eq!(&fixture.bytes[..4], b"\x7fELF");
        if !tool_available("readelf") {
            continue;
        }

        let dir = temporary_directory("relocations");
        let object = dir.join("forge.o");
        fs::write(&object, &fixture.bytes).expect("write object");
        let mut command = Command::new("readelf");
        command.args(["--wide", "-s", "-r"]).arg(&object);
        let output = successful_output(&mut command, "inspect C11d relocations");
        let report = String::from_utf8_lossy(&output.stdout);

        assert!(
            report.lines().any(|line| {
                line.contains(&fixture.module_initializer)
                    && line.contains("FUNC")
                    && line.contains("GLOBAL")
            }),
            "missing exported module initializer:\n{report}"
        );
        for initializer in &fixture.initializer_symbols {
            assert!(
                report.lines().any(|line| {
                    line.contains(initializer) && line.contains("FUNC") && line.contains("LOCAL")
                }),
                "missing local runtime initializer {initializer}:\n{report}"
            );
            let calls = report
                .lines()
                .filter(|line| line.contains(initializer) && !line.contains("FUNC"))
                .count();
            assert_eq!(
                calls, 1,
                "module initializer must call {initializer} exactly once:\n{report}"
            );
        }
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn c11d_links_and_executes_dependent_initializers_on_aarch64() {
    let fixture = emit(CraneliftTarget::Aarch64);
    let dir = temporary_directory("aarch64-run");
    let object = dir.join("forge.o");
    let harness = dir.join("harness.c");
    let executable = dir.join("forge-linked");
    fs::write(&object, &fixture.bytes).expect("write AArch64 object");

    let first = fixture.globals.get(&FIRST).expect("first symbol");
    let second = fixture.globals.get(&SECOND).expect("second symbol");
    let pair = fixture.globals.get(&PAIR).expect("pair symbol");
    let source = format!(
        "#include <stdint.h>\n\
extern void forge_module_init(void) __asm__(\"{}\");\n\
extern uint64_t forge_first __asm__(\"{}\");\n\
extern uint64_t forge_second __asm__(\"{}\");\n\
extern uint64_t forge_pair[2] __asm__(\"{}\");\n\
int main(void) {{\n\
    if (forge_first != 0 || forge_second != 0 || forge_pair[0] != 0 || forge_pair[1] != 0) return 1;\n\
    forge_module_init();\n\
    if (forge_first != 40) return 2;\n\
    if (forge_second != 40) return 3;\n\
    if (forge_pair[0] != 40 || forge_pair[1] != 40) return 4;\n\
    return 0;\n\
}}\n",
        fixture.module_initializer, first, second, pair
    );
    fs::write(&harness, source).expect("write C harness");

    let mut linker = Command::new("cc");
    linker
        .args(["-O0", "-no-pie"])
        .arg(&harness)
        .arg(&object)
        .arg("-o")
        .arg(&executable);
    successful_output(&mut linker, "link AArch64 C11d object");
    successful_output(
        &mut Command::new(&executable),
        "execute AArch64 C11d program",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn c11d_links_and_executes_dependent_initializers_on_riscv64() {
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

    let fixture = emit(CraneliftTarget::Riscv64);
    let first = fixture.globals.get(&FIRST).expect("first symbol");
    let second = fixture.globals.get(&SECOND).expect("second symbol");
    let pair = fixture.globals.get(&PAIR).expect("pair symbol");
    let source_text = format!(
        ".option nopic\n\
.option norelax\n\
.section .text\n\
.globl _start\n\
_start:\n\
    lla t0, {first}\n\
    ld t1, 0(t0)\n\
    bnez t1, fail\n\
    lla t0, {second}\n\
    ld t1, 0(t0)\n\
    bnez t1, fail\n\
    call {module_init}\n\
    lla t0, {first}\n\
    ld t1, 0(t0)\n\
    li t2, 40\n\
    bne t1, t2, fail\n\
    lla t0, {second}\n\
    ld t1, 0(t0)\n\
    bne t1, t2, fail\n\
    lla t0, {pair}\n\
    ld t1, 0(t0)\n\
    bne t1, t2, fail\n\
    ld t1, 8(t0)\n\
    bne t1, t2, fail\n\
    li a0, 0\n\
    li a7, 93\n\
    ecall\n\
fail:\n\
    li a0, 1\n\
    li a7, 93\n\
    ecall\n",
        module_init = fixture.module_initializer
    );

    let dir = temporary_directory("riscv64-run");
    let object = dir.join("forge.o");
    let source = dir.join("start.S");
    let start = dir.join("start.o");
    let executable = dir.join("forge-linked");
    fs::write(&object, &fixture.bytes).expect("write RV64 object");
    fs::write(&source, source_text).expect("write RV64 harness");

    let mut assembler = Command::new("riscv64-linux-gnu-as");
    assembler
        .args(["-march=rv64gc", "-mabi=lp64d", "-o"])
        .arg(&start)
        .arg(&source);
    successful_output(&mut assembler, "assemble RV64 C11d harness");

    let mut linker = Command::new("riscv64-linux-gnu-ld");
    linker
        .args(["--no-relax", "-e", "_start", "-o"])
        .arg(&executable)
        .arg(&start)
        .arg(&object);
    successful_output(&mut linker, "link RV64 C11d object");

    let mut run = Command::new("qemu-riscv64");
    run.arg(&executable);
    successful_output(&mut run, "execute linked RV64 C11d program");
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
        "forge-c11d-{label}-{}-{serial}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove stale C11d temp directory");
    }
    fs::create_dir_all(&path).expect("create C11d temp directory");
    path
}
