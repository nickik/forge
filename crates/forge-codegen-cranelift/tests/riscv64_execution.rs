use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
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

fn compile_choose() -> MachineCode {
    let backend = CraneliftBackend::riscv64().expect("RV64 backend");
    let (module, owner) = choose_module();
    let prepared = backend.prepare_module(&module).expect("verified RV64 CLIF");
    backend
        .emit_machine_code(&prepared, owner)
        .expect("RV64 machine code")
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

fn run_under_qemu(machine: &MachineCode, argument: u16) -> i32 {
    assert!(argument < 2048, "test launcher uses ADDI immediate");
    let image = rv64_elf_launcher(machine.bytes(), argument);
    let path = temporary_executable(argument);
    let mut file = fs::File::create(&path).expect("create RV64 ELF");
    file.write_all(&image).expect("write RV64 ELF");
    drop(file);

    let status = Command::new("qemu-riscv64")
        .arg(&path)
        .status()
        .expect("qemu-riscv64 must be installed when FORGE_RISCV64_EXECUTION is set");
    let _ = fs::remove_file(&path);
    status.code().expect("qemu terminated by signal")
}

fn temporary_executable(argument: u16) -> PathBuf {
    std::env::temp_dir().join(format!("forge-rv64-{}-{argument}.elf", std::process::id()))
}

fn rv64_elf_launcher(function: &[u8], argument: u16) -> Vec<u8> {
    const ELF_HEADER: usize = 64;
    const PROGRAM_HEADER: usize = 56;
    const CODE_OFFSET: usize = 0x1000;
    const BASE_ADDRESS: u64 = 0x1_0000;

    let wrapper = [
        encode_addi(10, 0, argument as i16),
        encode_jal(1, 12),
        encode_addi(17, 0, 93),
        0x0000_0073,
    ];

    let mut code = Vec::with_capacity(wrapper.len() * 4 + function.len());
    for instruction in wrapper {
        code.extend_from_slice(&instruction.to_le_bytes());
    }
    code.extend_from_slice(function);

    let mut elf = vec![0_u8; CODE_OFFSET];
    elf[0..4].copy_from_slice(b"\x7fELF");
    elf[4] = 2;
    elf[5] = 1;
    elf[6] = 1;
    put_u16(&mut elf, 16, 2);
    put_u16(&mut elf, 18, 243);
    put_u32(&mut elf, 20, 1);
    put_u64(&mut elf, 24, BASE_ADDRESS);
    put_u64(&mut elf, 32, ELF_HEADER as u64);
    put_u16(&mut elf, 52, ELF_HEADER as u16);
    put_u16(&mut elf, 54, PROGRAM_HEADER as u16);
    put_u16(&mut elf, 56, 1);

    let ph = ELF_HEADER;
    put_u32(&mut elf, ph, 1);
    put_u32(&mut elf, ph + 4, 5);
    put_u64(&mut elf, ph + 8, CODE_OFFSET as u64);
    put_u64(&mut elf, ph + 16, BASE_ADDRESS);
    put_u64(&mut elf, ph + 24, BASE_ADDRESS);
    put_u64(&mut elf, ph + 32, code.len() as u64);
    put_u64(&mut elf, ph + 40, code.len() as u64);
    put_u64(&mut elf, ph + 48, 0x1000);

    elf.extend_from_slice(&code);
    elf
}

fn encode_addi(rd: u32, rs1: u32, immediate: i16) -> u32 {
    let imm = (immediate as u16 as u32) & 0x0fff;
    (imm << 20) | (rs1 << 15) | (rd << 7) | 0x13
}

fn encode_jal(rd: u32, offset: i32) -> u32 {
    assert_eq!(offset & 1, 0);
    assert!((-1_048_576..=1_048_574).contains(&offset));
    let imm = offset as u32;
    let bit20 = (imm >> 20) & 0x1;
    let bits10_1 = (imm >> 1) & 0x3ff;
    let bit11 = (imm >> 11) & 0x1;
    let bits19_12 = (imm >> 12) & 0xff;
    (bit20 << 31) | (bits10_1 << 21) | (bit11 << 20) | (bits19_12 << 12) | (rd << 7) | 0x6f
}

fn put_u16(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put_u64(buffer: &mut [u8], offset: usize, value: u64) {
    buffer[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
