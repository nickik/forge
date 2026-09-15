mod module_linker;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::CraneliftBackend;
use forge_frontend::{
    ast::SourceFile, collect_type_definitions, lower_fir, lower_module, lower_resolved_bodies,
    parse_source, type_check_module, DefId, IntWidth, Ty,
};
use module_linker::{link_modules, ParsedLibrary};

#[derive(Debug)]
pub struct CompilerError(String);

impl CompilerError {
    fn message(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for CompilerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CompilerError {}

impl From<std::io::Error> for CompilerError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<forge_codegen_cranelift::BackendError> for CompilerError {
    fn from(error: forge_codegen_cranelift::BackendError) -> Self {
        Self(error.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryInput {
    name: String,
    path: PathBuf,
}

impl LibraryInput {
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }

    pub fn parse(spec: &str) -> Result<Self, CompilerError> {
        let (name, path) = spec.split_once('=').ok_or_else(|| {
            CompilerError::message(format!("invalid --library `{spec}`; expected NAME=PATH"))
        })?;
        if name.is_empty() || path.is_empty() {
            return Err(CompilerError::message(format!(
                "invalid --library `{spec}`; expected non-empty NAME=PATH"
            )));
        }
        Ok(Self::new(name, path))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Debug, Clone)]
pub struct CompiledProgram {
    object: Vec<u8>,
    main_owner: DefId,
    main_symbol: String,
    initializer_symbol: Option<String>,
}

impl CompiledProgram {
    pub fn object(&self) -> &[u8] {
        &self.object
    }

    pub fn main_owner(&self) -> DefId {
        self.main_owner
    }

    pub fn main_symbol(&self) -> &str {
        &self.main_symbol
    }

    pub fn initializer_symbol(&self) -> Option<&str> {
        self.initializer_symbol.as_deref()
    }
}

pub fn compile_source(source: &str) -> Result<CompiledProgram, CompilerError> {
    compile_source_with_library_sources(source, &[])
}

pub fn compile_source_with_library_sources(
    source: &str,
    libraries: &[(String, String)],
) -> Result<CompiledProgram, CompilerError> {
    let root = parse_ast("root source", source)?;
    let mut parsed_libraries = Vec::with_capacity(libraries.len());
    for (name, source) in libraries {
        parsed_libraries.push(ParsedLibrary {
            name: name.clone(),
            ast: parse_ast(&format!("library `{name}`"), source)?,
        });
    }
    let ast = link_modules(root, parsed_libraries)
        .map_err(|error| CompilerError::message(format!("module linking failed: {error}")))?;
    compile_ast(ast)
}

fn parse_ast(label: &str, source: &str) -> Result<SourceFile, CompilerError> {
    let parsed = parse_source(source);
    if parsed.ast.is_none() || !parsed.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "parse failed for {label}: {:?}",
            parsed.diagnostics
        )));
    }
    Ok(parsed.ast.expect("checked above"))
}

fn compile_ast(ast: SourceFile) -> Result<CompiledProgram, CompilerError> {
    let hir = lower_module(&ast);
    if !hir.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "HIR lowering failed: {:?}",
            hir.diagnostics
        )));
    }
    debug_assert!(
        hir.module.imports.is_empty(),
        "C12c module linker must consume all imports"
    );

    let bodies = lower_resolved_bodies(&ast, &hir.module);
    if !bodies.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "body HIR lowering failed: {:?}",
            bodies.diagnostics
        )));
    }

    let typed = type_check_module(&ast, &hir.module, &bodies);
    if !typed.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "type checking failed: {:?}",
            typed.diagnostics
        )));
    }

    let definitions = collect_type_definitions(&ast, &hir.module, &bodies, &typed);
    let fir = lower_fir(&bodies, &typed);
    if !fir.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "FIR lowering failed: {:?}",
            fir.diagnostics
        )));
    }

    let main_owner = hir
        .module
        .symbols
        .get("main")
        .and_then(|symbols| symbols.value_def)
        .ok_or_else(|| CompilerError::message("native executable requires fn main() -> i32"))?;
    let main = fir
        .module
        .functions
        .get(&main_owner)
        .ok_or_else(|| CompilerError::message("main did not lower to a FIR function"))?;
    if !main.params.is_empty()
        || main.return_type
            != (Ty::Int {
                signed: true,
                width: IntWidth::W32,
            })
    {
        return Err(CompilerError::message(
            "hosted C12 entry point must have signature fn main() -> i32",
        ));
    }

    let backend = CraneliftBackend::aarch64()?;
    let prepared = backend.prepare_module_with_types(&fir.module, &definitions)?;
    let initializer_owner = prepared.module_initializer_owner();
    let mut exports = vec![main_owner];
    if let Some(owner) = initializer_owner {
        exports.push(owner);
    }
    let plan = backend.plan_object_module_with_exports(&prepared, exports)?;
    let main_symbol = plan
        .symbol(main_owner)
        .ok_or_else(|| CompilerError::message("object plan omitted main"))?
        .name()
        .to_owned();
    let initializer_symbol = initializer_owner
        .map(|owner| {
            plan.symbol(owner)
                .map(|symbol| symbol.name().to_owned())
                .ok_or_else(|| CompilerError::message("object plan omitted module initializer"))
        })
        .transpose()?;
    let object = backend.emit_object(&prepared, &plan)?.into_bytes();

    Ok(CompiledProgram {
        object,
        main_owner,
        main_symbol,
        initializer_symbol,
    })
}

pub fn compile_file(path: &Path) -> Result<CompiledProgram, CompilerError> {
    compile_file_with_libraries(path, &[])
}

pub fn compile_file_with_libraries(
    path: &Path,
    libraries: &[LibraryInput],
) -> Result<CompiledProgram, CompilerError> {
    let source = fs::read_to_string(path).map_err(|error| {
        CompilerError::message(format!("cannot read {}: {error}", path.display()))
    })?;
    let mut sources = Vec::with_capacity(libraries.len());
    for library in libraries {
        let library_source = fs::read_to_string(library.path()).map_err(|error| {
            CompilerError::message(format!(
                "cannot read library {} from {}: {error}",
                library.name(),
                library.path().display()
            ))
        })?;
        sources.push((library.name().to_owned(), library_source));
    }
    compile_source_with_library_sources(&source, &sources)
}

pub fn check_file(path: &Path) -> Result<(), CompilerError> {
    check_file_with_libraries(path, &[])
}

pub fn check_file_with_libraries(
    path: &Path,
    libraries: &[LibraryInput],
) -> Result<(), CompilerError> {
    compile_file_with_libraries(path, libraries).map(|_| ())
}

pub fn emit_object_file(path: &Path, output: &Path) -> Result<(), CompilerError> {
    emit_object_file_with_libraries(path, output, &[])
}

pub fn emit_object_file_with_libraries(
    path: &Path,
    output: &Path,
    libraries: &[LibraryInput],
) -> Result<(), CompilerError> {
    let compiled = compile_file_with_libraries(path, libraries)?;
    fs::write(output, compiled.object()).map_err(|error| {
        CompilerError::message(format!("cannot write {}: {error}", output.display()))
    })
}

pub fn build_executable(path: &Path, output: &Path) -> Result<(), CompilerError> {
    build_executable_with_libraries(path, output, &[])
}

pub fn build_executable_with_libraries(
    path: &Path,
    output: &Path,
    libraries: &[LibraryInput],
) -> Result<(), CompilerError> {
    require_native_aarch64_linux()?;
    let compiled = compile_file_with_libraries(path, libraries)?;
    link_hosted(&compiled, output)
}

pub fn run_file(path: &Path, args: &[String]) -> Result<Output, CompilerError> {
    run_file_with_libraries(path, args, &[])
}

pub fn run_file_with_libraries(
    path: &Path,
    args: &[String],
    libraries: &[LibraryInput],
) -> Result<Output, CompilerError> {
    require_native_aarch64_linux()?;
    let dir = temporary_directory("run")?;
    let executable = dir.join("program");
    let result = (|| {
        let compiled = compile_file_with_libraries(path, libraries)?;
        link_hosted(&compiled, &executable)?;
        Command::new(&executable)
            .args(args)
            .output()
            .map_err(|error| {
                CompilerError::message(format!("cannot execute native program: {error}"))
            })
    })();
    let _ = fs::remove_dir_all(&dir);
    result
}

fn link_hosted(program: &CompiledProgram, output: &Path) -> Result<(), CompilerError> {
    let dir = temporary_directory("link")?;
    let object = dir.join("forge.o");
    let startup = dir.join("startup.c");
    fs::write(&object, program.object())?;
    fs::write(&startup, hosted_startup_source(program))?;

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let link = Command::new(&cc)
        .args(["-std=c11", "-O0", "-no-pie"])
        .arg(&startup)
        .arg(&object)
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|error| {
            CompilerError::message(format!("cannot start hosted linker {cc}: {error}"))
        })?;
    let _ = fs::remove_dir_all(&dir);
    if !link.status.success() {
        return Err(CompilerError::message(format!(
            "hosted link failed with {}\nstdout:\n{}\nstderr:\n{}",
            link.status,
            String::from_utf8_lossy(&link.stdout),
            String::from_utf8_lossy(&link.stderr)
        )));
    }
    Ok(())
}

fn hosted_startup_source(program: &CompiledProgram) -> String {
    let initializer = program
        .initializer_symbol()
        .map_or_else(String::new, |symbol| {
            format!("extern void {symbol}(void);\n")
        });
    let initialize = program
        .initializer_symbol()
        .map_or_else(String::new, |symbol| format!("    {symbol}();\n"));
    format!(
        "#include <stdint.h>\n#include <stdlib.h>\n\n\
         extern int32_t {main_symbol}(void);\n\
         {initializer}\n\
         __attribute__((noreturn)) void __forge_panic(const void *info) {{\n\
             (void)info;\n\
             abort();\n\
         }}\n\n\
         int main(void) {{\n\
         {initialize}\
             return (int){main_symbol}();\n\
         }}\n",
        main_symbol = program.main_symbol(),
    )
}

fn require_native_aarch64_linux() -> Result<(), CompilerError> {
    if cfg!(all(target_arch = "aarch64", target_os = "linux")) {
        Ok(())
    } else {
        Err(CompilerError::message(
            "C12 hosted linking/execution currently requires native AArch64 Linux",
        ))
    }
}

fn temporary_directory(label: &str) -> Result<PathBuf, CompilerError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("forge-c12-{label}-{}-{serial}", std::process::id()));
    if path.exists() {
        fs::remove_dir_all(&path)?;
    }
    fs::create_dir_all(&path)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE: &str = r#"
module test.native;
fn main() -> i32 {
    val x: i32 = 20 + 22;
    if (x == 42) { return 0; }
    return 1;
}
"#;

    const LIBRARY: &str = r#"
module math;
pub type Count = i32;
pub fn add_two(value: Count) -> Count { return value + 2; }
pub fn forty() -> Count { return 40; }
"#;

    const WITH_LIBRARY: &str = r#"
module test.library;
import math;
fn main() -> i32 {
    val answer: math.Count = math.add_two(math.forty());
    if (answer == 42) { return 0; }
    return 1;
}
"#;

    #[test]
    fn source_pipeline_emits_aarch64_elf_and_exports_main() {
        let compiled = compile_source(SIMPLE).expect("compile source");
        assert_eq!(&compiled.object()[..4], b"\x7fELF");
        assert!(compiled.main_symbol().starts_with("__forge_fn_"));
        assert!(compiled.initializer_symbol().is_none());
    }

    #[test]
    fn c12c_compiles_public_function_and_type_imports_semantically() {
        let compiled =
            compile_source_with_library_sources(WITH_LIBRARY, &[("math".into(), LIBRARY.into())])
                .expect("compile linked modules");
        assert_eq!(&compiled.object()[..4], b"\x7fELF");
    }

    #[test]
    fn c12c_rejects_private_imports() {
        let main = r#"
module test.private;
import secret;
fn main() -> i32 { return secret.hidden(); }
"#;
        let library = r#"
module secret;
fn hidden() -> i32 { return 0; }
"#;
        let error = compile_source_with_library_sources(main, &[("secret".into(), library.into())])
            .unwrap_err()
            .to_string();
        assert!(error.contains("private value `secret.hidden`"), "{error}");
    }

    #[test]
    fn hosted_startup_calls_forge_main_and_exposes_panic_hook() {
        let compiled = CompiledProgram {
            object: Vec::new(),
            main_owner: DefId(3),
            main_symbol: "__forge_fn_00000003".into(),
            initializer_symbol: Some("__forge_fn_00000009".into()),
        };
        let source = hosted_startup_source(&compiled);
        assert!(source.contains("__forge_fn_00000009();"));
        assert!(source.contains("return (int)__forge_fn_00000003();"));
        assert!(source.contains("__forge_panic"));
    }

    #[test]
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    fn links_and_executes_source_natively() {
        let dir = temporary_directory("test").expect("temp directory");
        let source = dir.join("main.fg");
        fs::write(&source, SIMPLE).expect("write source");
        let output = run_file(&source, &[]).expect("run source");
        assert!(
            output.status.success(),
            "status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    #[cfg(all(target_arch = "aarch64", target_os = "linux"))]
    fn c12c_links_and_executes_imported_library_natively() {
        let dir = temporary_directory("library-test").expect("temp directory");
        let source = dir.join("main.fg");
        let library = dir.join("math.fg");
        fs::write(&source, WITH_LIBRARY).expect("write source");
        fs::write(&library, LIBRARY).expect("write library");
        let libraries = vec![LibraryInput::new("math", &library)];
        let output = run_file_with_libraries(&source, &[], &libraries).expect("run source");
        assert!(
            output.status.success(),
            "status={} stdout={} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let _ = fs::remove_dir_all(dir);
    }
}
