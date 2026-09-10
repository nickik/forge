use std::{env, fs, path::Path, process};

use forge_frontend::parse_source;

mod manifest;
use manifest::{parse_suite, Suite, TestCase, TestKind};

const DEFAULT_MANIFEST: &str = "examples/conformance/suite.fdn";

#[derive(Debug)]
enum Outcome {
    Pass,
    Fail(String),
    Pending(&'static str),
}

fn main() {
    let manifest_path = parse_args();
    let manifest_text = fs::read_to_string(&manifest_path).unwrap_or_else(|error| {
        eprintln!(
            "forge-conformance: cannot read {}: {error}",
            manifest_path.display()
        );
        process::exit(2);
    });

    let suite = parse_suite(&manifest_text).unwrap_or_else(|error| {
        eprintln!(
            "forge-conformance: invalid {}: {error}",
            manifest_path.display()
        );
        process::exit(2);
    });

    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut pending = 0usize;

    println!(
        "Forge conformance suite :{} v{} (active: {})",
        suite.name,
        suite.version,
        format_active_kinds(&suite)
    );

    for test in &suite.tests {
        let path = root.join(&test.path);
        let outcome = execute_case(&suite, test, &path);
        match outcome {
            Outcome::Pass => {
                passed += 1;
                println!("PASS    {:8} {}", test.kind.as_str(), test.path.display());
            }
            Outcome::Pending(reason) => {
                pending += 1;
                println!(
                    "PENDING {:8} {} ({reason})",
                    test.kind.as_str(),
                    test.path.display()
                );
            }
            Outcome::Fail(reason) => {
                failed += 1;
                println!("FAIL    {:8} {}", test.kind.as_str(), test.path.display());
                for line in reason.lines() {
                    println!("        {line}");
                }
            }
        }
    }

    println!();
    println!("summary: {passed} passed; {failed} failed; {pending} pending");

    if failed != 0 {
        process::exit(1);
    }
}

fn parse_args() -> std::path::PathBuf {
    let mut args = env::args().skip(1);
    let first = args.next();
    if args.next().is_some() {
        usage();
    }

    match first.as_deref() {
        None => DEFAULT_MANIFEST.into(),
        Some("-h" | "--help") => usage(),
        Some(path) => path.into(),
    }
}

fn usage() -> ! {
    eprintln!("usage: forge-conformance [suite.fdn]");
    eprintln!("default: {DEFAULT_MANIFEST}");
    process::exit(2);
}

fn execute_case(suite: &Suite, test: &TestCase, path: &Path) -> Outcome {
    if !suite.active_kinds.contains(&test.kind) {
        return Outcome::Pending("kind not active in :active-kinds");
    }

    match test.kind {
        TestKind::Parse => execute_parse(path),
        TestKind::Negative => Outcome::Fail(format!(
            ":negative was activated for {} before a semantic rejection executor exists (expected :{})",
            test.path.display(),
            test.expected.as_deref().unwrap_or("unspecified")
        )),
        TestKind::Run => Outcome::Fail(format!(
            ":run was activated for {} before a codegen/execute executor exists (expected exit {})",
            test.path.display(),
            test.exit.unwrap_or(0)
        )),
    }
}

fn execute_parse(path: &Path) -> Outcome {
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return Outcome::Fail(format!("cannot read {}: {error}", path.display())),
    };

    // This is the same frontend entry point used by the `forge-parse` CLI.
    // Calling it in-process avoids one subprocess per fixture while exercising
    // exactly the same lexer and parser.
    let output = parse_source(&source);

    if output.ast.is_some() && output.diagnostics.is_empty() {
        return Outcome::Pass;
    }

    let mut reason = String::new();
    if output.ast.is_none() {
        reason.push_str("parser produced no AST");
    }

    for diagnostic in output.diagnostics {
        if !reason.is_empty() {
            reason.push('\n');
        }
        reason.push_str(&format!(
            "{}..{}: {}",
            diagnostic.span.start, diagnostic.span.end, diagnostic.message
        ));
    }

    Outcome::Fail(reason)
}

fn format_active_kinds(suite: &Suite) -> String {
    suite
        .active_kinds
        .iter()
        .map(|kind| format!(":{}", kind.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}
