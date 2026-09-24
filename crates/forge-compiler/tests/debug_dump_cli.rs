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
        dump.contains("\"answer\""),
        "FIR dump should preserve function names"
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
