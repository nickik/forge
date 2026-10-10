use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn compile(source: &str, mode: &str) -> std::process::Output {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "forge-sia-user-entry-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.fg"), source).unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"));
    command.arg(dir.join("main.fg"));
    if !mode.is_empty() {
        command.arg(mode);
    }
    let result = command
        .args(["--entry", "main", "--text-base", "0x00230000", "-o"])
        .arg(dir.join("image.bin"))
        .output()
        .unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    result
}

#[test]
fn user_entry_accepts_four_typed_startup_words() {
    let output = compile(
        "module app; fn main(a: u32,b: u32,c: u32,d: u32) -> i32 { return i32(a ^ b ^ c ^ d); }",
        "--user-image",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn user_entry_rejects_fifth_argument_and_non_word_types() {
    for source in [
        "module app; fn main(a: u32,b: u32,c: u32,d: u32,e: u32) -> i32 { return 0; }",
        "module app; fn main(a: i32) -> i32 { return a; }",
        "module app; fn main(a: str) -> i32 { return 0; }",
    ] {
        let output = compile(source, "--user-image");
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("zero to four u32 startup arguments")
        );
    }
}

#[test]
fn raw_trap_entry_keeps_its_two_word_limit() {
    let output = compile(
        "module app; fn main(a: u32) -> i32 { return i32(a); }",
        "--raw-image",
    );
    assert!(output.status.success());
    // Raw trap entry still rejects the larger user startup signature.
    let output = compile(
        "module app; fn main(a: u32,b: u32,c: u32,d: u32) -> i32 { return 0; }",
        "--raw-image",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("zero to two u32 arguments"));
}

#[test]
fn reset_rom_retains_zero_argument_entry_contract() {
    let output = compile("module app; fn main(a: u32) -> i32 { return i32(a); }", "");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fn main() -> i32"));
}

#[test]
fn user_entry_requires_i32_return() {
    let output = compile(
        "module app; fn main(a: u32) -> u32 { return a; }",
        "--user-image",
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("zero to four u32 startup arguments) -> i32"));
}
