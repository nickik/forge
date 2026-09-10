use std::{path::PathBuf, process::Command};

#[test]
fn forge_parse_cli_accepts_conformance_fixture() {
    let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = repository_root.join("examples/conformance/parse/01-tagged-union.fg");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-parse"))
        .arg("--json")
        .arg(&fixture)
        .output()
        .expect("forge-parse should start");

    assert!(
        output.status.success(),
        "forge-parse rejected {}:\nstdout:\n{}\nstderr:\n{}",
        fixture.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    let json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("forge-parse --json should emit JSON");
    assert!(json["ast"].is_object(), "CLI JSON should contain an AST");
    assert_eq!(
        json["diagnostics"].as_array().map(Vec::len),
        Some(0),
        "CLI JSON should contain no diagnostics"
    );
}

#[test]
fn forge_parse_cli_rejects_malformed_source() {
    let malformed = std::env::temp_dir().join(format!(
        "forge-cli-invalid-{}.fg",
        std::process::id()
    ));
    std::fs::write(&malformed, "module test.invalid; fn main( -> i32 { return 0; }")
        .expect("write malformed source");

    let output = Command::new(env!("CARGO_BIN_EXE_forge-parse"))
        .arg("--json")
        .arg(&malformed)
        .output()
        .expect("forge-parse should start");
    let _ = std::fs::remove_file(&malformed);

    assert!(
        !output.status.success(),
        "forge-parse must return non-zero for malformed source"
    );
}
