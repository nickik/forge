use std::path::PathBuf;
use std::process::{Command, Output};

fn fixture_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forge-sia32-user-image-{name}-{}",
        std::process::id()
    ))
}

fn run_user_image(source: &PathBuf, library: &PathBuf, output: &PathBuf) -> Output {
    Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(source)
        .args(["--library", &format!("cosmic.abi={}", library.display())])
        .args(["--entry", "system_task_entry"])
        .arg("--user-image")
        .args(["--text-base", "0x00200000"])
        .arg("-o")
        .arg(output)
        .output()
        .expect("forge-lighting-firmware should start")
}

#[test]
fn cosmic_user_image_contract_emits_deterministic_linked_bytes() {
    let root = fixture_dir("linked");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let library = root.join("abi.fg");
    let first_image = root.join("system-task-first.bin");
    let second_image = root.join("system-task-second.bin");
    std::fs::write(
        &source,
        r#"
module cosmic.system_task;
import cosmic.abi;

pub fn system_task_entry() -> i32 {
    return abi.answer();
}
"#,
    )
    .expect("write System Task fixture");
    std::fs::write(
        &library,
        r#"
module cosmic.abi;

pub fn answer() -> i32 {
    return 42;
}
"#,
    )
    .expect("write ABI fixture");

    let first = run_user_image(&source, &library, &first_image);
    let second = run_user_image(&source, &library, &second_image);

    assert!(
        first.status.success(),
        "first user-image compile failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second user-image compile failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(String::from_utf8_lossy(&first.stderr).contains("SIA32 Forge user image"));
    let first_bytes = std::fs::read(&first_image).expect("read first user image");
    let second_bytes = std::fs::read(&second_image).expect("read second user image");
    assert!(!first_bytes.is_empty(), "user image must contain linked text");
    assert_eq!(first_bytes, second_bytes, "user image must be deterministic");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_requires_an_explicit_virtual_address() {
    let root = fixture_dir("address");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn system_task_entry() -> i32 { return 0; }",
    )
    .expect("write System Task fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--entry", "system_task_entry", "--user-image"])
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--user-image requires an explicit --text-base user virtual address"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_rejects_boot_payload_embedding() {
    let root = fixture_dir("payload");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let payload = root.join("kernel.bin");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn system_task_entry() -> i32 { return 0; }",
    )
    .expect("write System Task fixture");
    std::fs::write(&payload, [0u8; 4]).expect("write payload fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--entry", "system_task_entry"])
        .arg("--user-image")
        .args(["--text-base", "0x00200000"])
        .arg("--embed-payload")
        .arg(&payload)
        .arg("0x1000")
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--user-image cannot embed a boot payload"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let _ = std::fs::remove_dir_all(root);
}
