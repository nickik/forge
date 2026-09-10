use std::{env, fs, process};

use forge_frontend::parse_source;

fn usage() -> ! {
    eprintln!("usage: forge-parse [--json] <file.fg>");
    process::exit(2);
}

fn main() {
    let mut json = false;
    let mut path = None;

    for arg in env::args().skip(1) {
        match arg.as_str() {
            "--json" => json = true,
            "-h" | "--help" => usage(),
            _ if path.is_none() => path = Some(arg),
            _ => usage(),
        }
    }

    let path = path.unwrap_or_else(|| usage());
    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("forge-parse: cannot read {path}: {error}");
        process::exit(2);
    });

    let result = parse_source(&source);

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&result).expect("AST serialization failed")
        );
    } else if let Some(ast) = &result.ast {
        println!("{ast:#?}");
        for diagnostic in &result.diagnostics {
            eprintln!(
                "{}..{}: {}",
                diagnostic.span.start, diagnostic.span.end, diagnostic.message
            );
        }
    } else {
        for diagnostic in &result.diagnostics {
            eprintln!(
                "{}..{}: {}",
                diagnostic.span.start, diagnostic.span.end, diagnostic.message
            );
        }
    }

    if result.diagnostics.is_empty() && result.ast.is_some() {
        process::exit(0);
    }
    process::exit(1);
}
