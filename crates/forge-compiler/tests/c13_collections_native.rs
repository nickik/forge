#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
use std::path::PathBuf;

#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
use forge_compiler::{run_file_with_libraries, LibraryInput};

#[test]
#[cfg(all(target_arch = "aarch64", target_os = "linux"))]
fn existing_bootstrap_collections_smoke_runs_natively() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let libraries = vec![
        LibraryInput::new(
            "forge_collections_bootstrap",
            root.join("packages/forge-collections-bootstrap/src/lib.fg"),
        ),
        LibraryInput::new(
            "std.collections.raw_u8",
            root.join("lib/std/collections/raw_u8.fg"),
        ),
        LibraryInput::new(
            "std.collections.raw_u64",
            root.join("lib/std/collections/raw_u64.fg"),
        ),
        LibraryInput::new(
            "std.collections.raw_usize",
            root.join("lib/std/collections/raw_usize.fg"),
        ),
        LibraryInput::new(
            "std.collections.raw_string",
            root.join("lib/std/collections/raw_string.fg"),
        ),
        LibraryInput::new("std.string", root.join("lib/std/string.fg")),
    ];
    let source = root.join("examples/collections-bootstrap-smoke/src/main.fg");
    let output = run_file_with_libraries(&source, &[], &libraries).expect("run collections smoke");
    assert!(
        output.status.success(),
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
