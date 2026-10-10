use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

fn compile(root: &str, library: &str) -> std::process::Output {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!("forge-sia-modules-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("root.fg"), root).unwrap();
    std::fs::write(dir.join("memory.fg"), library).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(dir.join("root.fg"))
        .args(["--raw-image", "--entry", "main", "--text-base", "0x00100000", "--library"])
        .arg(format!("memory={}", dir.join("memory.fg").display()))
        .arg("-o").arg(dir.join("image.bin")).output().unwrap();
    std::fs::remove_dir_all(dir).unwrap();
    result
}

#[test]
fn native_module_links_pointer_alias_and_internal_calls() {
    let output = compile("module app; import memory; fn main() -> i32 { return memory.read(); }",
        "module memory; type Word = *volatile u32; pub fn read() -> i32 { unsafe { val p: Word = Word(4096u32); return i32(*p); } }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn reachable_host_storage_placeholder_is_rejected() {
    let output = compile("module app; import memory; fn main() -> i32 { return memory.allocate(); }",
        "module memory; nfn __forge_raw_usize_create(slots: usize) -> usize { return 0; } pub fn allocate() -> i32 { return i32(__forge_raw_usize_create(:slots=2)); }");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("explicit storage provider"));
}

#[test]
fn unused_host_storage_does_not_enter_native_image() {
    let output = compile("module app; import memory; fn main() -> i32 { return 42; }",
        "module memory; nfn __forge_raw_usize_create(slots: usize) -> usize { return 0; } pub fn allocate() -> i32 { return i32(__forge_raw_usize_create(:slots=2)); }");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn raw_entry_accepts_two_architectural_words() {
    let output = compile("module app; import memory; fn main(selector: u32, frame: u32) -> u32 { return selector | frame; }", "module memory;");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn raw_entry_rejects_non_word_argument() {
    let output = compile("module app; import memory; fn main(frame: str) -> i32 { return 0; }", "module memory;");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("zero to two u32 arguments"));
}
