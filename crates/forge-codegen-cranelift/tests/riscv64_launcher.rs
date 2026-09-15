use std::fs;
use std::io::Write;
use std::process::Command;

#[test]
fn handcrafted_rv64_elf_exits_with_requested_status() {
    if std::env::var_os("FORGE_RISCV64_EXECUTION").is_none() {
        return;
    }
    for status in [0_u16, 2, 5, 10] {
        let (actual, trace) = run_direct_exit(status);
        assert_eq!(
            actual, status as i32,
            "qemu-riscv64 trace for requested status {status}:\n{trace}"
        );
    }
}

fn run_direct_exit(status: u16) -> (i32, String) {
    let image = direct_exit_elf(status);
    let path = std::env::temp_dir().join(format!(
        "forge-rv64-launcher-{}-{status}.elf",
        std::process::id()
    ));
    let mut file = fs::File::create(&path).expect("create RV64 ELF");
    file.write_all(&image).expect("write RV64 ELF");
    drop(file);
    let output = Command::new("qemu-riscv64")
        .arg("-strace")
        .arg(&path)
        .output()
        .expect("run qemu-riscv64");
    let _ = fs::remove_file(&path);
    (
        output.status.code().expect("qemu terminated by signal"),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn direct_exit_elf(status: u16) -> Vec<u8> {
    const ELF_HEADER: usize = 64;
    const PROGRAM_HEADER: usize = 56;
    const CODE_OFFSET: usize = 0x1000;
    const BASE_ADDRESS: u64 = 0x40_0000;

    let wrapper = [
        encode_addi(10, 0, status),
        encode_addi(17, 0, 93),
        0x0000_0073,
    ];
    let mut code = Vec::new();
    for instruction in wrapper {
        code.extend_from_slice(&instruction.to_le_bytes());
    }

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

fn encode_addi(rd: u32, rs1: u32, immediate: u16) -> u32 {
    ((immediate as u32 & 0x0fff) << 20) | (rs1 << 15) | (rd << 7) | 0x13
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
