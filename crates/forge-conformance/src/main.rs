use std::{
    collections::HashSet,
    env, fs,
    path::{Component, Path},
    process,
};

use forge_frontend::{
    lower_module, lower_resolved_bodies, parse_source, resolve_module_bodies, type_check_module,
};

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
        eprintln!("forge-conformance: cannot read {}: {error}", manifest_path.display());
        process::exit(2);
    });
    let suite = parse_suite(&manifest_text).unwrap_or_else(|error| {
        eprintln!("forge-conformance: invalid {}: {error}", manifest_path.display());
        process::exit(2);
    });
    validate_suite(&suite).unwrap_or_else(|error| {
        eprintln!("forge-conformance: invalid {}: {error}", manifest_path.display());
        process::exit(2);
    });

    let root = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut pending = 0usize;
    println!("Forge conformance suite :{} v{} (active: {})", suite.name, suite.version, format_active_kinds(&suite));

    for test in &suite.tests {
        let path = root.join(&test.path);
        match execute_case(&suite, test, &path) {
            Outcome::Pass => { passed += 1; println!("PASS    {:15} {}", test.kind.as_str(), test.path.display()); }
            Outcome::Pending(reason) => { pending += 1; println!("PENDING {:15} {} ({reason})", test.kind.as_str(), test.path.display()); }
            Outcome::Fail(reason) => {
                failed += 1;
                println!("FAIL    {:15} {}", test.kind.as_str(), test.path.display());
                for line in reason.lines() { println!("        {line}"); }
            }
        }
    }
    println!();
    println!("summary: {passed} passed; {failed} failed; {pending} pending");
    if failed != 0 { process::exit(1); }
}

fn parse_args() -> std::path::PathBuf {
    let mut args = env::args().skip(1);
    let first = args.next();
    if args.next().is_some() { usage(); }
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

fn validate_suite(suite: &Suite) -> Result<(), String> {
    if suite.name != "forge/conformance" { return Err(format!("unsupported :suite :{}; expected :forge/conformance", suite.name)); }
    if suite.version != 1 { return Err(format!("unsupported suite :version {}; expected 1", suite.version)); }
    let mut paths = HashSet::new();
    for test in &suite.tests {
        if test.path.as_os_str().is_empty() { return Err("test :path must not be empty".into()); }
        if test.path.is_absolute() || test.path.components().any(|c| !matches!(c, Component::Normal(_))) {
            return Err(format!("test :path must stay within the suite directory: {}", test.path.display()));
        }
        if !paths.insert(test.path.clone()) { return Err(format!("duplicate test :path {}", test.path.display())); }
        match test.kind {
            TestKind::Parse => if test.expected.is_some() || test.exit.is_some() { return Err(format!(":parse test {} must not specify :expect or :exit", test.path.display())); },
            TestKind::SyntaxNegative | TestKind::Negative => {
                if test.expected.is_none() { return Err(format!(":{} test {} requires :expect", test.kind.as_str(), test.path.display())); }
                if test.exit.is_some() { return Err(format!(":{} test {} must not specify :exit", test.kind.as_str(), test.path.display())); }
            }
            TestKind::Run => {
                if test.exit.is_none() { return Err(format!(":run test {} requires :exit", test.path.display())); }
                if test.expected.is_some() { return Err(format!(":run test {} must not specify :expect", test.path.display())); }
            }
        }
    }
    Ok(())
}

fn execute_case(suite: &Suite, test: &TestCase, path: &Path) -> Outcome {
    if !suite.active_kinds.contains(&test.kind) { return Outcome::Pending("kind not active in :active-kinds"); }
    match test.kind {
        TestKind::Parse => execute_parse(path),
        TestKind::SyntaxNegative => execute_syntax_negative(path),
        TestKind::Negative => execute_negative(test, path),
        TestKind::Run => Outcome::Fail(format!(":run was activated for {} before a codegen/execute executor exists (expected exit {})", test.path.display(), test.exit.unwrap_or(0))),
    }
}

fn read_source(path: &Path) -> Result<String, Outcome> {
    fs::read_to_string(path).map_err(|error| Outcome::Fail(format!("cannot read {}: {error}", path.display())))
}

fn execute_parse(path: &Path) -> Outcome {
    let source = match read_source(path) { Ok(source) => source, Err(outcome) => return outcome };
    let output = parse_source(&source);
    if output.ast.is_some() && output.diagnostics.is_empty() { Outcome::Pass } else { Outcome::Fail(format_parse_failure(output.ast.is_some(), output.diagnostics)) }
}

fn execute_syntax_negative(path: &Path) -> Outcome {
    let source = match read_source(path) { Ok(source) => source, Err(outcome) => return outcome };
    let output = parse_source(&source);
    if output.ast.is_none() || !output.diagnostics.is_empty() { Outcome::Pass } else { Outcome::Fail("source was expected to be rejected syntactically, but parsed cleanly".into()) }
}

fn execute_negative(test: &TestCase, path: &Path) -> Outcome {
    let expected = test.expected.as_deref().unwrap_or("unspecified");
    let source = match read_source(path) { Ok(source) => source, Err(outcome) => return outcome };
    let parsed = parse_source(&source);
    if parsed.ast.is_none() || !parsed.diagnostics.is_empty() {
        return Outcome::Fail(format!("semantic-negative source did not parse cleanly:\n{}", format_parse_failure(parsed.ast.is_some(), parsed.diagnostics)));
    }
    let ast = parsed.ast.expect("checked above");
    let items = lower_module(&ast);
    if !items.diagnostics.is_empty() {
        return Outcome::Fail(format!("item collection failed before semantics: {}", items.diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; ")));
    }

    if expected == "name/unresolved" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved.diagnostics.iter().any(|d| d.message.starts_with("unresolved ")) { Outcome::Pass }
        else { Outcome::Fail("expected unresolved-name diagnostic, but name resolution completed without one".into()) };
    }

    const IMPLEMENTED_TYPE_CODES: &[&str] = &[
        "type/mismatch",
        "type/distinct",
        "type/return",
        "call/duplicate-name",
        "call/unknown-name",
        "control/tail-call-required",
    ];
    if !IMPLEMENTED_TYPE_CODES.contains(&expected) {
        return Outcome::Pending("semantic stage for this expectation is not implemented yet");
    }

    let bodies = lower_resolved_bodies(&ast, &items.module);
    if !bodies.diagnostics.is_empty() {
        return Outcome::Fail(format!("resolved HIR lowering failed before type checking: {}", bodies.diagnostics.iter().map(|d| d.message.as_str()).collect::<Vec<_>>().join("; ")));
    }
    let typed = type_check_module(&ast, &items.module, &bodies);
    if typed.diagnostics.iter().any(|d| d.code == expected) {
        Outcome::Pass
    } else {
        let found = typed.diagnostics.iter().map(|d| format!("{}: {}", d.code, d.message)).collect::<Vec<_>>().join("; ");
        Outcome::Fail(format!("expected semantic diagnostic `{expected}`, found [{}]", found))
    }
}

fn format_parse_failure(has_ast: bool, diagnostics: Vec<forge_frontend::Diagnostic>) -> String {
    let mut reason = String::new();
    if !has_ast { reason.push_str("parser produced no AST"); }
    for diagnostic in diagnostics {
        if !reason.is_empty() { reason.push('\n'); }
        reason.push_str(&format!("{}..{}: {}", diagnostic.span.start, diagnostic.span.end, diagnostic.message));
    }
    reason
}

fn format_active_kinds(suite: &Suite) -> String {
    suite.active_kinds.iter().map(|kind| format!(":{}", kind.as_str())).collect::<Vec<_>>().join(" ")
}
