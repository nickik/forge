use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::CraneliftBackend;
use forge_frontend::{
    collect_type_definitions, lower_fir, lower_module, lower_resolved_bodies, parse_source,
    type_check_module, DefId, IntWidth, Ty,
};

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
    let parsed = parse_source(source);
    if parsed.ast.is_none() || !parsed.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "parse failed: {:?}",
            parsed.diagnostics
        )));
    }
    let ast = parsed.ast.expect("checked above");

    let hir = lower_module(&ast);
    if !hir.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "HIR lowering failed: {:?}",
            hir.diagnostics
        )));
    }
    if !hir.module.imports.is_empty() {
        return Err(CompilerError::message(
            "C12a native compiler supports one source module only; imported libraries require C12c",
        ));
    }

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
    let source = fs::read_to_string(path).map_err(|error| {
        CompilerError::message(format!("cannot read {}: {error}", path.display()))
    })?;
    compile_source(&source)
}

pub fn check_file(path: &Path) -> Result<(), CompilerError> {
    compile_file(path).map(|_| ())
}

pub fn emit_object_file(path: &Path, output: &Path) -> Result<(), CompilerError> {
    let compiled = compile_file(path)?;
    fs::write(output, compiled.object()).map_err(|error| {
        CompilerError::message(format!("cannot write {}: {error}", output.display()))
    })
}

pub fn build_executable(path: &Path, output: &Path) -> Result<(), CompilerError> {
    require_native_aarch64_linux()?;
    let compiled = compile_file(path)?;
    link_hosted(&compiled, output)
}

pub fn run_file(path: &Path, args: &[String]) -> Result<Output, CompilerError> {
    require_native_aarch64_linux()?;
    let dir = temporary_directory("run")?;
    let executable = dir.join("program");
    let result = (|| {
        let compiled = compile_file(path)?;
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
            "C12b hosted linking/execution currently requires native AArch64 Linux",
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

    #[test]
    fn source_pipeline_emits_aarch64_elf_and_exports_main() {
        let compiled = compile_source(SIMPLE).expect("compile source");
        assert_eq!(&compiled.object()[..4], b"\x7fELF");
        assert!(compiled.main_symbol().starts_with("__forge_fn_"));
        assert!(compiled.initializer_symbol().is_none());
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
}
