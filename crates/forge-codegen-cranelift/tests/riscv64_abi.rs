use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::process::Command;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn scalar_module(identity: bool, constant: u64) -> (FirModule, DefId) {
    let owner = DefId(0);
    let param = FirLocalId(0);
    let value = FirValueId(0);
    let ty = u64_ty();
    let instruction = if identity {
        FirInstruction {
            span: Span::new(0, 0),
            result: Some(value),
            kind: FirInstructionKind::Load {
                place: FirPlace::Local { local: param },
            },
        }
    } else {
        FirInstruction {
            span: Span::new(0, 0),
            result: Some(value),
            kind: FirInstructionKind::Const {
                value: FirConst::Integer {
                    text: constant.to_string(),
                },
            },
        }
    };
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
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![instruction],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, ty)]),
    };
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

fn execute(identity: bool, constant: u64, argument: u16) -> i32 {
    let backend = CraneliftBackend::riscv64().expect("RV64 backend");
    let (module, owner) = scalar_module(identity, constant);
    let prepared = backend.prepare_module(&module).expect("verified RV64 CLIF");
    let machine = backend
        .emit_machine_code(&prepared, owner)
        .expect("RV64 machine code");
    run(machine.bytes(), argument)
}

#[test]
fn qemu_wrapper_preserves_constant_return() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_some() {
        assert_eq!(execute(false, 2, 77), 2);
    }
}

#[test]
fn qemu_wrapper_passes_a0_and_returns_a0() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_some() {
        assert_eq!(execute(true, 0, 5), 5);
        assert_eq!(execute(true, 0, 10), 10);
    }
}

fn run(function: &[u8], argument: u16) -> i32 {
    let path = std::env::temp_dir().join(format!(
        "forge-rv64-abi-{}-{argument}.elf",
        std::process::id()
    ));
    let mut file = fs::File::create(&path).expect("create RV64 ELF");
    file.write_all(&elf(function, argument)).expect("write RV64 ELF");
    drop(file);
    let status = Command::new("qemu-riscv64")
        .arg(&path)
        .status()
        .expect("run qemu-riscv64");
    let _ = fs::remove_file(&path);
    status.code().expect("qemu terminated by signal")
}

fn elf(function: &[u8], argument: u16) -> Vec<u8> {
    const H: usize = 64;
    const PH: usize = 56;
    const OFF: usize = 0x1000;
    const BASE: u64 = 0x1_0000;
    let wrapper = [addi(10, 0, argument), jal(1, 12), addi(17, 0, 93), 0x73];
    let mut code = Vec::new();
    for instruction in wrapper {
        code.extend_from_slice(&instruction.to_le_bytes());
    }
    code.extend_from_slice(function);
    let mut out = vec![0; OFF];
    out[0..4].copy_from_slice(b"\x7fELF");
    out[4] = 2;
    out[5] = 1;
    out[6] = 1;
    u16_at(&mut out, 16, 2);
    u16_at(&mut out, 18, 243);
    u32_at(&mut out, 20, 1);
    u64_at(&mut out, 24, BASE);
    u64_at(&mut out, 32, H as u64);
    u16_at(&mut out, 52, H as u16);
    u16_at(&mut out, 54, PH as u16);
    u16_at(&mut out, 56, 1);
    u32_at(&mut out, H, 1);
    u32_at(&mut out, H + 4, 5);
    u64_at(&mut out, H + 8, OFF as u64);
    u64_at(&mut out, H + 16, BASE);
    u64_at(&mut out, H + 24, BASE);
    u64_at(&mut out, H + 32, code.len() as u64);
    u64_at(&mut out, H + 40, code.len() as u64);
    u64_at(&mut out, H + 48, 0x1000);
    out.extend_from_slice(&code);
    out
}

fn addi(rd: u32, rs1: u32, immediate: u16) -> u32 {
    ((immediate as u32 & 0xfff) << 20) | (rs1 << 15) | (rd << 7) | 0x13
}

fn jal(rd: u32, offset: i32) -> u32 {
    let imm = offset as u32;
    (((imm >> 20) & 1) << 31)
        | (((imm >> 1) & 0x3ff) << 21)
        | (((imm >> 11) & 1) << 20)
        | (((imm >> 12) & 0xff) << 12)
        | (rd << 7)
        | 0x6f
}

fn u16_at(b: &mut [u8], o: usize, v: u16) { b[o..o + 2].copy_from_slice(&v.to_le_bytes()); }
fn u32_at(b: &mut [u8], o: usize, v: u32) { b[o..o + 4].copy_from_slice(&v.to_le_bytes()); }
fn u64_at(b: &mut [u8], o: usize, v: u64) { b[o..o + 8].copy_from_slice(&v.to_le_bytes()); }
