use std::path::PathBuf;
use std::process;

use forge_compiler::{build_executable, check_file, emit_object_file, run_file};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Check,
    EmitObject,
    Build,
    Run,
}

fn usage() -> ! {
    eprintln!(
        "usage: forgec [--target aarch64-unknown-linux-gnu] [--platform host] \
         [--library NAME=PATH]... [--program-arg ARG]... \
         <--check|--emit-object|--build|--run> FILE [-o OUTPUT]"
    );
    process::exit(64);
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("forgec: {error}");
        process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let mut mode = None;
    let mut source = None;
    let mut output = None;
    let mut target = "aarch64-unknown-linux-gnu".to_owned();
    let mut platform = "host".to_owned();
    let mut libraries = Vec::new();
    let mut program_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => usage(),
            "--target" => target = args.next().unwrap_or_else(|| usage()),
            "--platform" => platform = args.next().unwrap_or_else(|| usage()),
            "--library" => libraries.push(args.next().unwrap_or_else(|| usage())),
            "--program-arg" => program_args.push(args.next().unwrap_or_else(|| usage())),
            "-o" | "--output" => {
                output = Some(PathBuf::from(args.next().unwrap_or_else(|| usage())))
            }
            "--check" => set_mode(&mut mode, Mode::Check),
            "--emit-object" => set_mode(&mut mode, Mode::EmitObject),
            "--build" => set_mode(&mut mode, Mode::Build),
            "--run" => set_mode(&mut mode, Mode::Run),
            _ if mode.is_some() && source.is_none() => source = Some(PathBuf::from(arg)),
            _ => usage(),
        }
    }

    let mode = mode.unwrap_or_else(|| usage());
    let source = source.unwrap_or_else(|| usage());
    if target != "aarch64-unknown-linux-gnu" && target != "aarch64" {
        return Err(format!("C12 supports only AArch64 Linux, not `{target}`").into());
    }
    if platform != "host" {
        return Err(format!("C12b supports only --platform host, not `{platform}`").into());
    }
    if !libraries.is_empty() {
        return Err(
            "C12a is intentionally single-module; --library support arrives in C12c".into(),
        );
    }
    if mode != Mode::Run && !program_args.is_empty() {
        return Err("--program-arg is valid only with --run".into());
    }

    match mode {
        Mode::Check => check_file(&source)?,
        Mode::EmitObject => {
            let output = output.unwrap_or_else(|| source.with_extension("o"));
            emit_object_file(&source, &output)?;
        }
        Mode::Build => {
            let output = output.unwrap_or_else(|| source.with_extension(""));
            build_executable(&source, &output)?;
        }
        Mode::Run => {
            if output.is_some() {
                return Err("-o/--output is not valid with --run".into());
            }
            let result = run_file(&source, &program_args)?;
            use std::io::Write;
            std::io::stdout().write_all(&result.stdout)?;
            std::io::stderr().write_all(&result.stderr)?;
            process::exit(result.status.code().unwrap_or(1));
        }
    }
    Ok(())
}

fn set_mode(mode: &mut Option<Mode>, value: Mode) {
    if mode.replace(value).is_some() {
        usage();
    }
}
