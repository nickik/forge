use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use forge_compiler::{
    build_executable_with_libraries, build_executable_with_libraries_and_entry,
    check_file_with_libraries, emit_object_file_with_libraries,
    emit_object_file_with_libraries_and_entry, run_file_with_libraries,
    run_file_with_libraries_and_entry, LibraryInput,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Check,
    EmitObject,
    Build,
    Run,
}

struct TemporarySource {
    path: PathBuf,
}

impl TemporarySource {
    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TemporarySource {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn usage() -> ! {
    eprintln!(
        "usage: forgec [--target aarch64-unknown-linux-gnu] [--platform host] \
         [--library NAME=PATH]... [--entry NAME] [--program-arg ARG]... \
         <--check|--emit-object|--build|--run> FILE [-o OUTPUT]"
    );
    process::exit(64);
}

fn implicit_provider_source(
    source: &Path,
    libraries: &[LibraryInput],
) -> Result<Option<TemporarySource>, Box<dyn std::error::Error>> {
    if !libraries
        .iter()
        .any(|library| library.name() == "std.string")
    {
        return Ok(None);
    }

    let text = fs::read_to_string(source)?;
    if text.contains("import std.string;") || text.contains("module std.string;") {
        return Ok(None);
    }

    let module_start = text
        .find("module ")
        .ok_or("Forge source has no module declaration")?;
    let module_end = text[module_start..]
        .find(';')
        .map(|offset| module_start + offset + 1)
        .ok_or("Forge module declaration has no terminating semicolon")?;

    let mut rewritten = String::with_capacity(text.len() + 20);
    rewritten.push_str(&text[..module_end]);
    rewritten.push_str("\nimport std.string;");
    rewritten.push_str(&text[module_end..]);

    let path = std::env::temp_dir().join(format!(
        "forgec-{}-implicit-hosted-providers.fg",
        process::id()
    ));
    fs::write(&path, rewritten)?;
    Ok(Some(TemporarySource { path }))
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
    let mut entry = None;
    let mut library_specs = Vec::new();
    let mut program_args = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => usage(),
            "--target" => target = args.next().unwrap_or_else(|| usage()),
            "--platform" => platform = args.next().unwrap_or_else(|| usage()),
            "--library" => library_specs.push(args.next().unwrap_or_else(|| usage())),
            "--entry" => entry = Some(args.next().unwrap_or_else(|| usage())),
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
        return Err(format!("C14 supports only AArch64 Linux, not `{target}`").into());
    }
    if platform != "host" {
        return Err(format!("C14 supports only --platform host, not `{platform}`").into());
    }
    if mode != Mode::Run && !program_args.is_empty() {
        return Err("--program-arg is valid only with --run".into());
    }
    if mode == Mode::Check && entry.is_some() {
        return Err(
            "--entry is not needed with --check; checks do not require an entry point".into(),
        );
    }
    let libraries = library_specs
        .iter()
        .map(|spec| LibraryInput::parse(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let implicit_source = implicit_provider_source(&source, &libraries)?;
    let compile_source = implicit_source
        .as_ref()
        .map(TemporarySource::path)
        .unwrap_or(&source);

    match mode {
        Mode::Check => check_file_with_libraries(compile_source, &libraries)?,
        Mode::EmitObject => {
            let output = output.unwrap_or_else(|| source.with_extension("o"));
            if let Some(entry) = entry.as_deref() {
                emit_object_file_with_libraries_and_entry(
                    compile_source,
                    &output,
                    &libraries,
                    entry,
                )?;
            } else {
                emit_object_file_with_libraries(compile_source, &output, &libraries)?;
            }
        }
        Mode::Build => {
            let output = output.unwrap_or_else(|| source.with_extension(""));
            if let Some(entry) = entry.as_deref() {
                build_executable_with_libraries_and_entry(
                    compile_source,
                    &output,
                    &libraries,
                    entry,
                )?;
            } else {
                build_executable_with_libraries(compile_source, &output, &libraries)?;
            }
        }
        Mode::Run => {
            if output.is_some() {
                return Err("-o/--output is not valid with --run".into());
            }
            let result = if let Some(entry) = entry.as_deref() {
                run_file_with_libraries_and_entry(compile_source, &program_args, &libraries, entry)?
            } else {
                run_file_with_libraries(compile_source, &program_args, &libraries)?
            };
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
