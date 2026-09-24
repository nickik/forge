use std::{path::PathBuf, process::Command};

#[test]
fn dump_fir_is_deterministic_verified_json_on_stdout() {
    let source = std::env::temp_dir().join(format!("forgec-dump-fir-{}.fg", std::process::id()));
    std::fs::write(
        &source,
        r#"
module test.dump;
fn answer(value: u32) -> u32 { return value + 1u32; }
"#,
    )
    .expect("write FIR dump fixture");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forgec"))
            .arg("--dump-fir")
            .arg(&source)
            .output()
            .expect("forgec should start")
    };
    let first = run();
    let second = run();
    let _ = std::fs::remove_file(&source);

    assert!(
        first.status.success(),
        "first dump failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second dump failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(
        first.stdout, second.stdout,
        "FIR dump must be deterministic"
    );
    assert!(
        first.stderr.is_empty(),
        "successful dump must keep stderr clean"
    );

    let dump = String::from_utf8(first.stdout).expect("FIR dump should be UTF-8 JSON");
    assert!(dump.starts_with("{\n"), "FIR dump should be pretty JSON");
    assert!(
        dump.ends_with("\n"),
        "CLI should terminate the dump with a newline"
    );
    assert!(
        dump.contains("\"functions\""),
        "FIR dump should include functions"
    );
    assert!(
        dump.contains("\"instruction\": \"binary\""),
        "FIR dump should contain lowered instructions"
    );
}

#[test]
fn dump_fir_rejects_output_paths() {
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/hello.fg");
    let output = Command::new(env!("CARGO_BIN_EXE_forgec"))
        .args(["--dump-fir", "-o", "ignored.json"])
        .arg(source)
        .output()
        .expect("forgec should start");

    assert!(!output.status.success(), "--dump-fir must reject -o");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("the dump is written to stdout"),
        "unexpected diagnostic: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn dump_typed_hir_is_deterministic_resolved_json_on_stdout() {
    let source =
        std::env::temp_dir().join(format!("forgec-dump-typed-hir-{}.fg", std::process::id()));
    std::fs::write(
        &source,
        r#"
module test.typed_dump;
fn increment(value: u32) -> u32 { return value + 1u32; }
fn answer() -> u32 { return increment(41u32); }
"#,
    )
    .expect("write typed-HIR dump fixture");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forgec"))
            .arg("--dump-typed-hir")
            .arg(&source)
            .output()
            .expect("forgec should start")
    };
    let first = run();
    let second = run();
    let _ = std::fs::remove_file(&source);

    assert!(
        first.status.success(),
        "first dump failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second dump failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());

    let dump = String::from_utf8(first.stdout).expect("typed-HIR dump should be UTF-8 JSON");
    assert!(dump.starts_with("{\n"));
    assert!(dump.ends_with("\n"));
    assert!(dump.contains("\"functions\""));
    assert!(dump.contains("\"expr\": \"resolved_call\""));
    assert!(dump.contains("\"diagnostics\": []"));
}

#[test]
fn dump_abi_is_deterministic_and_exposes_direct_and_indirect_plans() {
    let source = std::env::temp_dir().join(format!("forgec-dump-abi-{}.fg", std::process::id()));
    std::fs::write(
        &source,
        r#"
module test.abi_dump;
struct Pair { left: u32; right: u32; }
struct Big { a: u64; b: u64; c: u64; d: u64; e: u64; }
fn pair(value: Pair) -> Pair { return value; }
fn big(value: Big) -> Big { return value; }
"#,
    )
    .expect("write ABI dump fixture");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forgec"))
            .arg("--dump-abi")
            .arg(&source)
            .output()
            .expect("forgec should start")
    };
    let first = run();
    let second = run();
    let _ = std::fs::remove_file(&source);

    assert!(
        first.status.success(),
        "first dump failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second dump failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());

    let dump = String::from_utf8(first.stdout).expect("ABI dump should be UTF-8 JSON");
    assert!(dump.starts_with("{\n"));
    assert!(dump.ends_with("\n"));
    assert!(dump.contains("\"target\": \"aarch64-unknown-linux-gnu\""));
    assert!(dump.contains("\"passing\": \"direct\""));
    assert!(dump.contains("\"passing\": \"indirect\""));
    assert!(dump.contains("\"kind\": \"integer\""));
}

#[test]
fn dump_clif_is_deterministic_production_aarch64_ir() {
    let source = std::env::temp_dir().join(format!("forgec-dump-clif-{}.fg", std::process::id()));
    std::fs::write(
        &source,
        r#"
module test.clif_dump;
fn answer(value: u32) -> u32 { return value + 1u32; }
"#,
    )
    .expect("write CLIF dump fixture");

    let run = || {
        Command::new(env!("CARGO_BIN_EXE_forgec"))
            .arg("--dump-clif")
            .arg(&source)
            .output()
            .expect("forgec should start")
    };
    let first = run();
    let second = run();
    let _ = std::fs::remove_file(&source);

    assert!(
        first.status.success(),
        "first dump failed:\n{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second dump failed:\n{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    assert!(first.stderr.is_empty());

    let dump = String::from_utf8(first.stdout).expect("CLIF dump should be UTF-8 text");
    assert!(dump.starts_with("; Forge function DefId("));
    assert!(dump.ends_with("\n"));
    assert!(dump.contains("function "));
    assert!(dump.contains("iadd"));
}
