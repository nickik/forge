use std::{
    collections::HashSet,
    env, fs,
    path::{Component, Path, PathBuf},
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

#[derive(Debug, PartialEq, Eq)]
struct Options {
    manifest_path: PathBuf,
    require_no_pending: bool,
}

fn main() {
    let Options {
        manifest_path,
        require_no_pending,
    } = parse_args();
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
    validate_suite(&suite).unwrap_or_else(|error| {
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
        match execute_case(&suite, test, &path) {
            Outcome::Pass => {
                passed += 1;
                println!("PASS    {:15} {}", test.kind.as_str(), test.path.display());
            }
            Outcome::Pending(reason) => {
                pending += 1;
                println!(
                    "PENDING {:15} {} ({reason})",
                    test.kind.as_str(),
                    test.path.display()
                );
            }
            Outcome::Fail(reason) => {
                failed += 1;
                println!("FAIL    {:15} {}", test.kind.as_str(), test.path.display());
                for line in reason.lines() {
                    println!("        {line}");
                }
            }
        }
    }
    println!();
    println!("summary: {passed} passed; {failed} failed; {pending} pending");
    if failed != 0 || (require_no_pending && pending != 0) {
        process::exit(1);
    }
}

fn parse_args() -> Options {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        usage();
    }
    parse_options(args).unwrap_or_else(|error| {
        eprintln!("forge-conformance: {error}");
        usage();
    })
}

fn parse_options(args: impl IntoIterator<Item = String>) -> Result<Options, String> {
    let mut manifest_path = None;
    let mut require_no_pending = false;
    for arg in args {
        match arg.as_str() {
            "--require-no-pending" => require_no_pending = true,
            option if option.starts_with('-') => {
                return Err(format!("unknown option `{option}`"));
            }
            path => {
                if manifest_path.replace(PathBuf::from(path)).is_some() {
                    return Err("only one suite manifest may be supplied".into());
                }
            }
        }
    }
    Ok(Options {
        manifest_path: manifest_path.unwrap_or_else(|| DEFAULT_MANIFEST.into()),
        require_no_pending,
    })
}

fn usage() -> ! {
    eprintln!("usage: forge-conformance [--require-no-pending] [suite.fdn]");
    eprintln!("default: {DEFAULT_MANIFEST}");
    process::exit(2);
}

fn validate_suite(suite: &Suite) -> Result<(), String> {
    if !matches!(
        suite.name.as_str(),
        "forge/conformance" | "forge/spec-examples"
    ) {
        return Err(format!(
            "unsupported :suite :{}; expected :forge/conformance or :forge/spec-examples",
            suite.name
        ));
    }
    if suite.version != 1 {
        return Err(format!(
            "unsupported suite :version {}; expected 1",
            suite.version
        ));
    }
    let requires_spec_mapping = suite.name == "forge/spec-examples";
    let mut paths = HashSet::new();
    for test in &suite.tests {
        if test.path.as_os_str().is_empty() {
            return Err("test :path must not be empty".into());
        }
        if test.path.is_absolute()
            || test
                .path
                .components()
                .any(|c| !matches!(c, Component::Normal(_)))
        {
            return Err(format!(
                "test :path must stay within the suite directory: {}",
                test.path.display()
            ));
        }
        if !paths.insert(test.path.clone()) {
            return Err(format!("duplicate test :path {}", test.path.display()));
        }
        if requires_spec_mapping
            && !matches!(test.spec.as_deref(), Some(section) if !section.trim().is_empty())
        {
            return Err(format!(
                "spec-example test {} requires a non-empty :spec mapping",
                test.path.display()
            ));
        }
        match test.kind {
            TestKind::Parse | TestKind::Check => {
                if test.expected.is_some() || test.exit.is_some() {
                    return Err(format!(
                        ":{} test {} must not specify :expect or :exit",
                        test.kind.as_str(),
                        test.path.display()
                    ));
                }
            }
            TestKind::SyntaxNegative | TestKind::Negative => {
                if test.expected.is_none() {
                    return Err(format!(
                        ":{} test {} requires :expect",
                        test.kind.as_str(),
                        test.path.display()
                    ));
                }
                if test.exit.is_some() {
                    return Err(format!(
                        ":{} test {} must not specify :exit",
                        test.kind.as_str(),
                        test.path.display()
                    ));
                }
            }
            TestKind::Run => {
                if test.exit.is_none() {
                    return Err(format!(":run test {} requires :exit", test.path.display()));
                }
                if test.expected.is_some() {
                    return Err(format!(
                        ":run test {} must not specify :expect",
                        test.path.display()
                    ));
                }
            }
        }
    }
    Ok(())
}

fn execute_case(suite: &Suite, test: &TestCase, path: &Path) -> Outcome {
    if !suite.active_kinds.contains(&test.kind) {
        return Outcome::Pending("kind not active in :active-kinds");
    }
    match test.kind {
        TestKind::Parse => execute_parse(path),
        TestKind::Check => execute_check(path),
        TestKind::SyntaxNegative => execute_syntax_negative(test, path),
        TestKind::Negative => execute_negative(test, path),
        TestKind::Run => execute_run(test, path),
    }
}

fn execute_check(path: &Path) -> Outcome {
    match forge_compiler::check_file(path) {
        Ok(()) => Outcome::Pass,
        Err(error) => Outcome::Fail(format!("production compiler check failed: {error}")),
    }
}

fn execute_run(test: &TestCase, path: &Path) -> Outcome {
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    {
        let output = match forge_compiler::run_file(path, &[]) {
            Ok(output) => output,
            Err(error) => return Outcome::Fail(format!("native execution failed: {error}")),
        };
        let expected = test.exit.unwrap_or(0);
        match output.status.code() {
            Some(actual) if actual == expected => Outcome::Pass,
            Some(actual) => Outcome::Fail(format!(
                "expected exit {expected}, got {actual}\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
            None => Outcome::Fail(format!(
                "native program terminated without an exit code\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )),
        }
    }
    #[cfg(not(all(target_arch = "aarch64", target_os = "linux")))]
    {
        let _ = (test, path);
        Outcome::Pending("native :run executor currently requires AArch64 Linux")
    }
}

fn read_source(path: &Path) -> Result<String, Outcome> {
    fs::read_to_string(path)
        .map_err(|error| Outcome::Fail(format!("cannot read {}: {error}", path.display())))
}

fn execute_parse(path: &Path) -> Outcome {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let output = parse_source(&source);
    if output.ast.is_some() && output.diagnostics.is_empty() {
        Outcome::Pass
    } else {
        Outcome::Fail(format_parse_failure(
            output.ast.is_some(),
            output.diagnostics,
        ))
    }
}

fn execute_syntax_negative(test: &TestCase, path: &Path) -> Outcome {
    let source = match read_source(path) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let output = parse_source(&source);
    syntax_negative_outcome(test, output)
}

fn syntax_negative_outcome(test: &TestCase, output: forge_frontend::ParseOutput) -> Outcome {
    if output.ast.is_some() && output.diagnostics.is_empty() {
        return Outcome::Fail(
            "source was expected to be rejected syntactically, but parsed cleanly".into(),
        );
    }

    let expected = test.expected.as_deref().unwrap_or("unspecified");
    const CODED_EXPECTATIONS: &[&str] = &[
        "syntax/extern-deferred",
        "syntax/switch-removed",
        "syntax/initializer-required",
        "syntax/empty-capture-list",
        "syntax/check-metadata-removed",
        "syntax/assignment-target",
        "syntax/enum-variant",
        "syntax/tagged-variant",
    ];
    if !CODED_EXPECTATIONS.contains(&expected) {
        return Outcome::Pass;
    }
    if output
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == expected)
    {
        Outcome::Pass
    } else {
        let found = output
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        Outcome::Fail(format!(
            "expected syntax diagnostic `{expected}`, found [{found}]"
        ))
    }
}

fn execute_negative(test: &TestCase, path: &Path) -> Outcome {
    let expected = test.expected.as_deref().unwrap_or("unspecified");
    let source = match read_source(path) {
        Ok(source) => source,
        Err(outcome) => return outcome,
    };
    let parsed = parse_source(&source);
    if parsed.ast.is_none() || !parsed.diagnostics.is_empty() {
        return Outcome::Fail(format!(
            "semantic-negative source did not parse cleanly:\n{}",
            format_parse_failure(parsed.ast.is_some(), parsed.diagnostics)
        ));
    }
    let ast = parsed.ast.expect("checked above");
    let items = lower_module(&ast);
    if !items.diagnostics.is_empty() {
        return Outcome::Fail(format!(
            "item collection failed before semantics: {}",
            items
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    if expected == "name/unresolved" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("unresolved "))
        {
            Outcome::Pass
        } else {
            Outcome::Fail(
                "expected unresolved-name diagnostic, but name resolution completed without one"
                    .into(),
            )
        };
    }

    if expected == "name/qualified-variant-required" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("name/qualified-variant-required:"))
        {
            Outcome::Pass
        } else {
            Outcome::Fail(
                "expected qualified-variant diagnostic, but bare variant was not identified".into(),
            )
        };
    }

    const IMPLEMENTED_TYPE_CODES: &[&str] = &[
        "type/mismatch",
        "type/distinct",
        "type/return",
        "type/index-on-type",
        "type/unknown-field",
        "type/missing-field",
        "pattern/type",
        "pattern/or-binding-type",
        "pattern/refutable-binding",
        "call/duplicate-name",
        "call/unknown-name",
        "control/tail-call-required",
        "type/declaration-default",
        "match/non-exhaustive",
        "match/unreachable-arm",
    ];
    if !IMPLEMENTED_TYPE_CODES.contains(&expected) {
        return Outcome::Pending("semantic stage for this expectation is not implemented yet");
    }

    let bodies = lower_resolved_bodies(&ast, &items.module);
    if !bodies.diagnostics.is_empty() {
        return Outcome::Fail(format!(
            "resolved HIR lowering failed before type checking: {}",
            bodies
                .diagnostics
                .iter()
                .map(|d| d.message.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let typed = type_check_module(&ast, &items.module, &bodies);
    if typed.diagnostics.iter().any(|d| d.code == expected) {
        Outcome::Pass
    } else {
        let found = typed
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.code, d.message))
            .collect::<Vec<_>>()
            .join("; ");
        Outcome::Fail(format!(
            "expected semantic diagnostic `{expected}`, found [{}]",
            found
        ))
    }
}

fn format_parse_failure(has_ast: bool, diagnostics: Vec<forge_frontend::Diagnostic>) -> String {
    let mut reason = String::new();
    if !has_ast {
        reason.push_str("parser produced no AST");
    }
    for diagnostic in diagnostics {
        if !reason.is_empty() {
            reason.push('\n');
        }
        reason.push_str(&format!(
            "{}..{}: {}: {}",
            diagnostic.span.start, diagnostic.span.end, diagnostic.code, diagnostic.message
        ));
    }
    reason
}

fn format_active_kinds(suite: &Suite) -> String {
    suite
        .active_kinds
        .iter()
        .map(|kind| format!(":{}", kind.as_str()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn options_default_to_the_repository_suite_and_allow_strict_pending_mode() {
        assert_eq!(
            parse_options(args(&[])).unwrap(),
            Options {
                manifest_path: DEFAULT_MANIFEST.into(),
                require_no_pending: false,
            }
        );
        assert_eq!(
            parse_options(args(&["--require-no-pending", "custom.fdn"])).unwrap(),
            Options {
                manifest_path: "custom.fdn".into(),
                require_no_pending: true,
            }
        );
        assert!(parse_options(args(&["one.fdn", "two.fdn"])).is_err());
        assert!(parse_options(args(&["--unknown"])).is_err());
    }

    #[test]
    fn spec_example_suites_require_section_mappings() {
        let mapped = parse_suite(
            r#"{:suite :forge/spec-examples
                :version 1
                :active-kinds [:check]
                :tests [{:path #path "check/value.fg" :kind :check :spec "§6 declarations"}]}"#,
        )
        .unwrap();
        assert!(validate_suite(&mapped).is_ok());

        let unmapped = parse_suite(
            r#"{:suite :forge/spec-examples
                :version 1
                :active-kinds [:check]
                :tests [{:path #path "check/value.fg" :kind :check}]}"#,
        )
        .unwrap();
        assert!(validate_suite(&unmapped).is_err());
    }

    #[test]
    fn coded_syntax_expectations_require_the_exact_parser_category() {
        let test = TestCase {
            path: "unused.fg".into(),
            kind: TestKind::SyntaxNegative,
            spec: None,
            expected: Some("syntax/switch-removed".into()),
            exit: None,
        };
        let matching = forge_frontend::ParseOutput {
            ast: None,
            diagnostics: vec![forge_frontend::Diagnostic {
                span: forge_frontend::ast::Span::new(0, 6),
                code: "syntax/switch-removed".into(),
                message: "reserved".into(),
            }],
        };
        let mismatching = forge_frontend::ParseOutput {
            ast: None,
            diagnostics: vec![forge_frontend::Diagnostic {
                span: forge_frontend::ast::Span::new(0, 6),
                code: "syntax/parse".into(),
                message: "generic parse failure".into(),
            }],
        };

        assert!(matches!(syntax_negative_outcome(&test, matching), Outcome::Pass));
        assert!(matches!(
            syntax_negative_outcome(&test, mismatching),
            Outcome::Fail(_)
        ));
    }
}
