#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

use forge_compiler::{run_file_with_libraries, LibraryInput};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .to_path_buf()
}

fn standard_libraries() -> Vec<LibraryInput> {
    let root = workspace_root();
    [
        ("core", "lib/core.fg"),
        ("std.args", "lib/std/args.fg"),
        ("std.console", "lib/std/console.fg"),
        ("std.fs", "lib/std/fs.fg"),
        ("std.lock", "lib/std/lock.fg"),
        ("std.string", "lib/std/string.fg"),
        ("std.time", "lib/std/time.fg"),
    ]
    .into_iter()
    .map(|(name, path)| LibraryInput::new(name, root.join(path)))
    .collect()
}

fn temporary_directory() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "forge-c14-hosted-std-{}-{serial}",
        std::process::id()
    ));
    if path.exists() {
        fs::remove_dir_all(&path).expect("remove stale test directory");
    }
    fs::create_dir_all(&path).expect("create test directory");
    path
}

#[test]
fn hosted_std_providers_cover_args_time_files_locking_and_string_helpers() {
    let dir = temporary_directory();
    let source = dir.join("main.fg");
    let database = dir.join("database.txt");
    fs::write(
        &source,
        r#"
module test.c14.hosted_std;

import std.args;
import std.console;
import std.fs;
import std.lock;
import std.string;
import std.time;

fn main() -> i32 {
    if (args.count() != 2) { return 10; }
    val path: str = args.get(0);
    val payload: str = args.get(1);
    if (payload != "hello") { return 11; }

    val started: u64 = time.monotonic_us();

    val file_lock: lock.FileLock = lock.acquire_exclusive(path);
    fs.write_text(path, payload);
    if (fs.read_text(path) != "hello") { return 12; }
    fs.append_text(path, "!");
    lock.release(file_lock);

    if (fs.read_text(path) != "hello!") { return 13; }
    if (string.concat("hello", "!") != "hello!") { return 14; }
    if (string.byte_len("hello") != 5) { return 15; }
    if (string.byte_at("hello", 1) != 101) { return 16; }
    if (string.parse_u64("184467") != 184467) { return 17; }
    if (string.parse_usize("4096") != 4096) { return 18; }
    if (string.from_u64(42) != "42") { return 19; }
    if (string.from_usize(64) != "64") { return 20; }

    val lines: str = "first\nsecond\n";
    if (string.line_count(lines) != 2) { return 21; }
    if (string.line_at(lines, 0) != "first") { return 22; }
    if (string.line_at(lines, 1) != "second") { return 23; }
    if (!string.has_tab("key\tvalue")) { return 24; }
    if (string.before_tab("key\tvalue") != "key") { return 25; }
    if (string.after_tab("key\tvalue") != "value") { return 26; }

    val finished: u64 = time.monotonic_us();
    if (finished < started) { return 27; }

    console.write("hosted-std-ok\n");
    return 0;
}
"#,
    )
    .expect("write Forge test program");

    let output = run_file_with_libraries(
        &source,
        &[database.to_string_lossy().into_owned(), "hello".to_owned()],
        &standard_libraries(),
    )
    .expect("compile and run hosted std program");

    assert!(
        output.status.success(),
        "status={} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"hosted-std-ok\n");
    assert_eq!(fs::read(&database).expect("read database"), b"hello!");

    fs::remove_dir_all(dir).expect("remove test directory");
}
