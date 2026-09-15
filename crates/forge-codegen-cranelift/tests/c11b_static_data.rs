use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::{CraneliftBackend, CraneliftTarget, GlobalStorageClass};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal, FirInstruction,
    FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span,
    StaticGlobalInitializer, StaticGlobalInitializerTable, StaticSymbol, StaticValue, Ty,
    TypeDefinition, TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition,
};

const FUNCTION: DefId = DefId(1);
const RO_SCALAR: DefId = DefId(10);
const RO_ARRAY: DefId = DefId(11);
const BSS: DefId = DefId(12);
const DATA_FN: DefId = DefId(13);
const DATA_GLOBAL: DefId = DefId(14);
const RO_STRUCT: DefId = DefId(15);
const RO_FN: DefId = DefId(16);
const STRUCT_TY: DefId = DefId(100);

fn int_ty(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn u8_ty() -> Ty {
    int_ty(IntWidth::W8)
}

fn u16_ty() -> Ty {
    int_ty(IntWidth::W16)
}

fn u64_ty() -> Ty {
    int_ty(IntWidth::W64)
}

fn function_ty() -> Ty {
    Ty::Function {
        params: Vec::new(),
        result: Box::new(u64_ty()),
        named_arguments: false,
    }
}

fn return_nine() -> FirFunction {
    let span = Span::new(0, 0);
    let value = FirValueId(0);
    FirFunction {
        owner: FUNCTION,
        params: Vec::new(),
        return_type: u64_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(value),
                kind: FirInstructionKind::Const {
                    value: FirConst::Integer { text: "9".into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u64_ty())]),
    }
}

fn fixture() -> (FirModule, TypeDefinitionTable, StaticGlobalInitializerTable) {
    let array_ty = Ty::Array {
        element: Box::new(u16_ty()),
        length: Some(3),
    };
    let pointer_ty = Ty::Pointer {
        volatile: false,
        inner: Box::new(u64_ty()),
    };

    let mut module = FirModule::default();
    module.functions.insert(FUNCTION, return_nine());
    module.globals.insert(
        RO_SCALAR,
        FirGlobal {
            owner: RO_SCALAR,
            ty: u64_ty(),
            constant: Some(ConstValue::Integer {
                value: 0x1122_3344_5566_7788,
            }),
        },
    );
    for (owner, ty) in [
        (RO_ARRAY, array_ty),
        (BSS, u64_ty()),
        (DATA_FN, function_ty()),
        (DATA_GLOBAL, pointer_ty),
        (RO_STRUCT, Ty::Nominal(STRUCT_TY)),
        (RO_FN, function_ty()),
    ] {
        module.globals.insert(
            owner,
            FirGlobal {
                owner,
                ty,
                constant: None,
            },
        );
    }

    let definitions = TypeDefinitionTable::from([(
        STRUCT_TY,
        TypeDefinition {
            owner: STRUCT_TY,
            kind: TypeDefinitionKind::Struct {
                fields: vec![
                    TypeFieldDefinition {
                        name: "a".into(),
                        ty: u8_ty(),
                        declaration_index: 0,
                    },
                    TypeFieldDefinition {
                        name: "b".into(),
                        ty: u64_ty(),
                        declaration_index: 1,
                    },
                    TypeFieldDefinition {
                        name: "c".into(),
                        ty: u16_ty(),
                        declaration_index: 2,
                    },
                ],
            },
        },
    )]);

    let integer = |value| StaticValue::Scalar(ConstValue::Integer { value });
    let static_initializers = StaticGlobalInitializerTable::from([
        (
            RO_ARRAY,
            StaticGlobalInitializer {
                value: StaticValue::Array(vec![integer(1), integer(0x2233), integer(0x4455)]),
                writable: false,
            },
        ),
        (
            DATA_FN,
            StaticGlobalInitializer {
                value: StaticValue::Address {
                    target: StaticSymbol::Function(FUNCTION),
                    addend: 0,
                },
                writable: true,
            },
        ),
        (
            DATA_GLOBAL,
            StaticGlobalInitializer {
                value: StaticValue::Address {
                    target: StaticSymbol::Global(BSS),
                    addend: 0,
                },
                writable: true,
            },
        ),
        (
            RO_STRUCT,
            StaticGlobalInitializer {
                value: StaticValue::Aggregate {
                    variant: None,
                    fields: BTreeMap::from([
                        ("a".into(), integer(0x11)),
                        ("b".into(), integer(0x2233_4455_6677_8899)),
                        ("c".into(), integer(0xaabb)),
                    ]),
                },
                writable: false,
            },
        ),
        (
            RO_FN,
            StaticGlobalInitializer {
                value: StaticValue::Address {
                    target: StaticSymbol::Function(FUNCTION),
                    addend: 0,
                },
                writable: false,
            },
        ),
    ]);

    (module, definitions, static_initializers)
}

fn prepare(target: CraneliftTarget) -> forge_codegen_cranelift::PreparedModule {
    let (module, definitions, static_initializers) = fixture();
    CraneliftBackend::new(target)
        .expect("backend")
        .prepare_module_with_static_initializers(&module, &definitions, &static_initializers)
        .expect("C11b fixture should prepare")
}

fn emit(target: CraneliftTarget) -> Vec<u8> {
    let backend = CraneliftBackend::new(target).expect("backend");
    let prepared = prepare(target);
    backend
        .emit_object_with_exports(
            &prepared,
            [
                FUNCTION,
                RO_SCALAR,
                RO_ARRAY,
                BSS,
                DATA_FN,
                DATA_GLOBAL,
                RO_STRUCT,
                RO_FN,
            ],
        )
        .expect("C11b object should emit")
        .into_bytes()
}

#[test]
fn c11b_serializes_scalars_aggregates_and_relocations_from_c9_layout() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let prepared = prepare(target);

        let scalar = prepared.global(RO_SCALAR).expect("scalar global");
        assert_eq!(scalar.storage(), GlobalStorageClass::ReadOnlyData);
        assert_eq!(
            scalar.static_data().expect("scalar bytes").bytes(),
            &0x1122_3344_5566_7788u64.to_le_bytes()
        );

        let array = prepared.global(RO_ARRAY).expect("array global");
        assert_eq!(array.storage(), GlobalStorageClass::ReadOnlyData);
        assert_eq!(
            array.static_data().expect("array bytes").bytes(),
            &[0x01, 0x00, 0x33, 0x22, 0x55, 0x44]
        );

        let structure = prepared.global(RO_STRUCT).expect("struct global");
        let bytes = structure.static_data().expect("struct bytes").bytes();
        let layout = structure.layout();
        assert_eq!(bytes.len() as u64, layout.size);
        assert_eq!(bytes[layout.field("a").expect("a").offset as usize], 0x11);
        assert_eq!(
            read_le(bytes, layout.field("b").expect("b").offset, 8),
            0x2233_4455_6677_8899
        );
        assert_eq!(
            read_le(bytes, layout.field("c").expect("c").offset, 2),
            0xaabb
        );

        let bss = prepared.global(BSS).expect("bss global");
        assert_eq!(bss.storage(), GlobalStorageClass::ZeroFill);
        assert!(bss.static_data().is_none());

        for (owner, target_symbol, storage) in [
            (
                DATA_FN,
                StaticSymbol::Function(FUNCTION),
                GlobalStorageClass::WritableData,
            ),
            (
                DATA_GLOBAL,
                StaticSymbol::Global(BSS),
                GlobalStorageClass::WritableData,
            ),
            (
                RO_FN,
                StaticSymbol::Function(FUNCTION),
                GlobalStorageClass::ReadOnlyData,
            ),
        ] {
            let global = prepared.global(owner).expect("relocated global");
            assert_eq!(global.storage(), storage);
            let relocations = global.static_data().expect("static data").relocations();
            assert_eq!(relocations.len(), 1);
            assert_eq!(relocations[0].target(), target_symbol);
            assert_eq!(relocations[0].width(), 8);
        }
    }
}

#[test]
fn c11b_emits_deterministic_elf_sections_symbols_alignment_and_data_relocations() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let first = emit(target);
        let second = emit(target);
        assert_eq!(
            first, second,
            "{target:?} C11b object must be deterministic"
        );
        assert_eq!(&first[..4], b"\x7fELF");

        if !tool_available("readelf") {
            continue;
        }
        let dir = temporary_directory("inspect");
        let object = dir.join("forge.o");
        fs::write(&object, &first).expect("write object");
        let mut command = Command::new("readelf");
        command.args(["--wide", "-S", "-s", "-r"]).arg(&object);
        let output = successful_output(&mut command, "inspect C11b object");
        let report = String::from_utf8_lossy(&output.stdout);

        for section in [".rodata", ".rela.rodata", ".data", ".rela.data", ".bss"] {
            assert!(report.contains(section), "missing {section}:\n{report}");
        }
        let rodata_line = section_line(&report, ".rodata");
        let data_line = section_line(&report, ".data");
        let bss_line = section_line(&report, ".bss");
        assert!(rodata_line.contains("PROGBITS"), "{rodata_line}");
        assert!(data_line.contains("PROGBITS"), "{data_line}");
        assert!(bss_line.contains("NOBITS"), "{bss_line}");
        assert_eq!(section_alignment(rodata_line), 8, "{rodata_line}");
        assert_eq!(section_alignment(data_line), 8, "{data_line}");
        assert_eq!(section_alignment(bss_line), 8, "{bss_line}");

        for symbol in [
            "__forge_global_0000000a",
            "__forge_global_0000000b",
            "__forge_global_0000000c",
            "__forge_global_0000000d",
            "__forge_global_0000000e",
            "__forge_global_0000000f",
            "__forge_global_00000010",
        ] {
            assert!(
                report
                    .lines()
                    .any(|line| line.contains(symbol) && line.contains("OBJECT")),
                "missing STT_OBJECT symbol {symbol}:\n{report}"
            );
        }
        assert!(
            report
                .lines()
                .any(|line| line.contains("__forge_fn_00000001") && line.contains("FUNC")),
            "missing function symbol:\n{report}"
        );
        let _ = fs::remove_dir_all(dir);
    }
}

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn c11b_links_and_executes_aarch64_static_data() {
    let dir = temporary_directory("aarch64-run");
    let object = dir.join("forge.o");
    let harness = dir.join("harness.c");
    let executable = dir.join("forge-linked");
    fs::write(&object, emit(CraneliftTarget::Aarch64)).expect("write AArch64 object");
    fs::write(
        &harness,
        r#"#include <stdint.h>
typedef uint64_t (*fn0)(void);
extern uint64_t __forge_fn_00000001(void);
extern const uint64_t __forge_global_0000000a;
extern const uint16_t __forge_global_0000000b[3];
extern uint64_t __forge_global_0000000c;
extern fn0 __forge_global_0000000d;
extern uint64_t *__forge_global_0000000e;
extern fn0 const __forge_global_00000010;
int main(void) {
    if (__forge_global_0000000a != UINT64_C(0x1122334455667788)) return 1;
    if (__forge_global_0000000b[0] != 1 || __forge_global_0000000b[1] != 0x2233 || __forge_global_0000000b[2] != 0x4455) return 2;
    if (__forge_global_0000000c != 0) return 3;
    if (__forge_global_0000000d != __forge_fn_00000001) return 4;
    if (__forge_global_00000010 != __forge_fn_00000001) return 5;
    if (__forge_fn_00000001() != 9) return 6;
    if (__forge_global_0000000e != &__forge_global_0000000c) return 7;
    *__forge_global_0000000e = 77;
    if (__forge_global_0000000c != 77) return 8;
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
    successful_output(&mut linker, "link AArch64 C11b object");
    successful_output(
        &mut Command::new(&executable),
        "execute AArch64 C11b program",
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn c11b_links_and_executes_riscv64_static_data_under_qemu() {
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
    la t0, __forge_global_0000000a
    ld t1, 0(t0)
    li t2, 0x1122334455667788
    bne t1, t2, fail

    la t0, __forge_global_0000000b
    lhu t1, 0(t0)
    li t2, 1
    bne t1, t2, fail
    lhu t1, 2(t0)
    li t2, 0x2233
    bne t1, t2, fail
    lhu t1, 4(t0)
    li t2, 0x4455
    bne t1, t2, fail

    la t0, __forge_global_0000000c
    ld t1, 0(t0)
    bnez t1, fail

    la t0, __forge_global_0000000d
    ld t1, 0(t0)
    la t2, __forge_fn_00000001
    bne t1, t2, fail

    la t0, __forge_global_00000010
    ld t1, 0(t0)
    bne t1, t2, fail

    la t0, __forge_global_0000000e
    ld t1, 0(t0)
    la t2, __forge_global_0000000c
    bne t1, t2, fail

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
    successful_output(&mut assembler, "assemble RV64 C11b harness");

    let mut linker = Command::new("riscv64-linux-gnu-ld");
    linker.arg("-o").arg(&executable).arg(&start).arg(&object);
    successful_output(&mut linker, "link RV64 C11b object");

    let mut run = Command::new("qemu-riscv64");
    run.arg(&executable);
    successful_output(&mut run, "execute linked RV64 C11b program");
    let _ = fs::remove_dir_all(dir);
}

fn read_le(bytes: &[u8], offset: u64, width: usize) -> u64 {
    let start = offset as usize;
    bytes[start..start + width]
        .iter()
        .enumerate()
        .fold(0u64, |value, (index, byte)| {
            value | (u64::from(*byte) << (index * 8))
        })
}

fn section_line<'a>(report: &'a str, name: &str) -> &'a str {
    report
        .lines()
        .find(|line| line.split_whitespace().any(|field| field == name))
        .unwrap_or_else(|| panic!("missing section line for {name}:\n{report}"))
}

fn section_alignment(line: &str) -> u64 {
    line.split_whitespace()
        .last()
        .expect("section alignment")
        .parse()
        .expect("numeric section alignment")
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
        "forge-c11b-{label}-{}-{serial}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove stale C11b temp directory");
    }
    fs::create_dir_all(&path).expect("create C11b temp directory");
    path
}
