use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "forge-sia32-user-image-{name}-{}",
        std::process::id()
    ))
}

fn run_user_image(source: &Path, library: &Path, output: &Path) -> Output {
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
    assert!(
        !first_bytes.is_empty(),
        "user image must contain linked text"
    );
    assert_eq!(
        first_bytes, second_bytes,
        "user image must be deterministic"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_relocates_calls_for_requested_text_base() {
    let root = fixture_dir("text-base");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let low_image = root.join("system-task-low.bin");
    let high_image = root.join("system-task-high.bin");
    std::fs::write(
        &source,
        r#"
module cosmic.system_task;

fn answer() -> i32 {
    return 42;
}

pub fn system_task_entry() -> i32 {
    return answer();
}
"#,
    )
    .expect("write linked System Task fixture");

    for (text_base, image) in [("0x00200000", &low_image), ("0x00300000", &high_image)] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
            .arg(&source)
            .args(["--entry", "system_task_entry"])
            .arg("--user-image")
            .args(["--text-base", text_base])
            .arg("-o")
            .arg(image)
            .output()
            .expect("forge-lighting-firmware should start");
        assert!(
            output.status.success(),
            "user-image compile at {text_base} failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let low_bytes = std::fs::read(&low_image).expect("read low-base user image");
    let high_bytes = std::fs::read(&high_image).expect("read high-base user image");
    assert_eq!(low_bytes.len(), high_bytes.len());
    let (low_words, low_remainder) = low_bytes.as_chunks::<4>();
    let (high_words, high_remainder) = high_bytes.as_chunks::<4>();
    assert!(low_remainder.is_empty());
    assert!(high_remainder.is_empty());
    let relocated_words = low_words
        .iter()
        .zip(high_words)
        .enumerate()
        .filter_map(|(index, (low, high))| {
            let low = u32::from_le_bytes(*low);
            let high = u32::from_le_bytes(*high);
            (low.checked_add(0x0010_0000) == Some(high)).then_some(index)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        relocated_words.len(),
        1,
        "one direct-call literal must track the requested text base"
    );
    let relocated_range = relocated_words[0] * 4..relocated_words[0] * 4 + 4;
    for (index, (low, high)) in low_bytes.iter().zip(&high_bytes).enumerate() {
        if !relocated_range.contains(&index) {
            assert_eq!(low, high, "non-relocation byte changed at offset {index}");
        }
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn m28_system_task_fixture_emits_deterministic_headerless_user_image() {
    let root = fixture_dir("m28-system-task");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/m28-5-syscall-r1.fg");
    let first_image = root.join("system-task-first.bin");
    let second_image = root.join("system-task-second.bin");

    for image in [&first_image, &second_image] {
        let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
            .arg(&source)
            .args(["--entry", "system_task_entry"])
            .arg("--user-image")
            .args(["--text-base", "0x00200000"])
            .arg("-o")
            .arg(image)
            .output()
            .expect("forge-lighting-firmware should start");
        assert!(
            output.status.success(),
            "M28 System Task compile failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let first_bytes = std::fs::read(&first_image).expect("read first M28 System Task image");
    let second_bytes = std::fs::read(&second_image).expect("read second M28 System Task image");
    assert!(
        !first_bytes.is_empty(),
        "user image must contain linked text"
    );
    assert_eq!(
        first_bytes, second_bytes,
        "checked-in M28 System Task image must be deterministic"
    );
    assert!(
        !first_bytes.starts_with(b"; Generated by forge-lighting-firmware"),
        "user image must be headerless linked bytes"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_defaults_to_a_binary_output_name() {
    let root = fixture_dir("default-output");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let binary = source.with_extension("bin");
    let assembly = source.with_extension("lighting.s");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn system_task_entry() -> i32 { return 0; }",
    )
    .expect("write System Task fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--entry", "system_task_entry"])
        .arg("--user-image")
        .args(["--text-base", "0x00200000"])
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(
        output.status.success(),
        "user-image compile failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(binary.exists(), "default user image was not emitted");
    assert!(
        !assembly.exists(),
        "binary user image must not use an assembly filename"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_rejects_float_fir_before_emission() {
    let root = fixture_dir("float-boundary");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let image = root.join("system-task.bin");
    std::fs::write(
        &source,
        r#"
module cosmic.system_task;

pub fn system_task_entry() -> i32 {
    val left: f32 = 1.5f32;
    val right: f32 = 2.0f32;
    val sum: f32 = left + right;
    if (sum > 0.0f32) { return 0i32; }
    return 1i32;
}
"#,
    )
    .expect("write System Task fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--entry", "system_task_entry"])
        .arg("--user-image")
        .args(["--text-base", "0x00200000"])
        .arg("-o")
        .arg(&image)
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("floating point on SIA32 (deferred)"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!image.exists(), "rejected float image must not be emitted");

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
fn reset_rom_contract_rejects_an_explicit_default_text_base() {
    let root = fixture_dir("reset-rom-text-base");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("firmware.fg");
    let image = root.join("firmware.s");
    std::fs::write(
        &source,
        "module test.firmware; pub fn main() -> i32 { return 0; }",
    )
    .expect("write firmware fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--text-base", "0xffff0014"])
        .arg("-o")
        .arg(&image)
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--text-base requires --raw-image"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!image.exists(), "rejected reset ROM must not emit output");

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_requires_an_explicit_entry() {
    let root = fixture_dir("entry");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let image = root.join("system-task.bin");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn main() -> i32 { return 0; }",
    )
    .expect("write System Task fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .arg("--user-image")
        .args(["--text-base", "0x00200000"])
        .arg("-o")
        .arg(&image)
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--user-image requires an explicit --entry function"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !image.exists(),
        "rejected user entry must not emit an image"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn cosmic_user_image_contract_rejects_raw_image_mode_in_either_order() {
    let root = fixture_dir("exclusive-mode");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn system_task_entry() -> i32 { return 0; }",
    )
    .expect("write System Task fixture");

    for (name, modes) in [
        ("raw-first", ["--raw-image", "--user-image"]),
        ("user-first", ["--user-image", "--raw-image"]),
    ] {
        let image = root.join(format!("{name}.bin"));
        let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
            .arg(&source)
            .args(["--entry", "system_task_entry"])
            .args(modes)
            .args(["--text-base", "0x00200000"])
            .arg("-o")
            .arg(&image)
            .output()
            .expect("forge-lighting-firmware should start");

        assert!(!output.status.success(), "{name} unexpectedly succeeded");
        assert!(
            String::from_utf8_lossy(&output.stderr)
                .contains("--raw-image and --user-image are mutually exclusive"),
            "unexpected {name} diagnostic: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!image.exists(), "rejected image mode must not emit output");
    }

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn raw_image_contract_rejects_silently_ignored_boot_payload() {
    let root = fixture_dir("raw-payload");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("firmware.fg");
    let payload = root.join("payload.bin");
    let image = root.join("firmware.bin");
    std::fs::write(
        &source,
        "module test.firmware; pub fn main() -> i32 { return 0; }",
    )
    .expect("write firmware fixture");
    std::fs::write(&payload, [1u8, 2, 3, 4]).expect("write payload fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .arg("--raw-image")
        .arg("--embed-payload")
        .arg(&payload)
        .arg("0x1000")
        .arg("-o")
        .arg(&image)
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("--embed-payload requires reset-ROM output"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!image.exists(), "rejected raw image must not emit output");

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

#[test]
fn cosmic_user_image_contract_rejects_raw_trap_entry_signature() {
    let root = fixture_dir("trap-entry");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create fixture directory");
    let source = root.join("system_task.fg");
    let image = root.join("system-task.bin");
    std::fs::write(
        &source,
        "module cosmic.system_task; pub fn m28_trap_entry(cause: u32) -> i32 { return 0; }",
    )
    .expect("write System Task fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-lighting-firmware"))
        .arg(&source)
        .args(["--entry", "m28_trap_entry", "--user-image"])
        .args(["--text-base", "0x00200000"])
        .arg("-o")
        .arg(&image)
        .output()
        .expect("forge-lighting-firmware should start");

    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("must have signature fn m28_trap_entry() -> i32"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !image.exists(),
        "rejected user entry must not emit an image"
    );

    let _ = std::fs::remove_dir_all(root);
}
