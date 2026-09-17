mod hosted_runtime;
mod module_linker;

use hosted_runtime::hosted_provider_source;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    BinaryOp, ConstValue, FirConst, FirFunction, FirGlobal, FirInstructionKind, FirModule,
    StaticGlobalInitializer, StaticGlobalInitializerTable, StaticSymbol, StaticValue,
};
use forge_frontend::{
    ast::{DeclKind, SourceFile},
    collect_type_definitions, lower_fir, lower_module, lower_resolved_bodies, parse_source,
    type_check_module, DefId, IntWidth, Ty,
};
use module_linker::{link_modules, ParsedLibrary};

const CHECK_ENTRY: &str = "__forge_c12c_c14_check_entry";

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
    console_symbol: Option<String>,
    hosted_providers: BTreeMap<String, String>,
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

    pub fn console_symbol(&self) -> Option<&str> {
        self.console_symbol.as_deref()
    }

    pub fn hosted_provider_symbols(&self) -> &BTreeMap<String, String> {
        &self.hosted_providers
    }
}

pub fn compile_source(source: &str) -> Result<CompiledProgram, CompilerError> {
    compile_source_with_library_sources(source, &[])
}

pub fn compile_source_with_library_sources(
    source: &str,
    libraries: &[(String, String)],
) -> Result<CompiledProgram, CompilerError> {
    compile_source_with_library_sources_for_entry(source, libraries, "main", None)
}

pub fn compile_source_with_library_sources_and_entry(
    source: &str,
    libraries: &[(String, String)],
    entry: &str,
) -> Result<CompiledProgram, CompilerError> {
    compile_source_with_library_sources_for_entry(source, libraries, entry, Some(entry))
}

fn compile_source_with_library_sources_for_entry(
    source: &str,
    libraries: &[(String, String)],
    entry: &str,
    external_entry_symbol: Option<&str>,
) -> Result<CompiledProgram, CompilerError> {
    if entry.is_empty() {
        return Err(CompilerError::message(
            "native entry point name must not be empty",
        ));
    }
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
    compile_ast(ast, entry, external_entry_symbol)
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

fn compile_ast(
    ast: SourceFile,
    entry_name: &str,
    external_entry_symbol: Option<&str>,
) -> Result<CompiledProgram, CompilerError> {
    let provider_names = ast
        .declarations
        .iter()
        .filter_map(|declaration| match &declaration.kind.kind {
            DeclKind::Function(function) => provider_intrinsic_name(&function.name)
                .map(|intrinsic| (function.name.clone(), intrinsic.to_owned())),
            _ => None,
        })
        .collect::<BTreeMap<_, _>>();

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
    let mut provider_owners = BTreeMap::new();
    for (symbol_name, intrinsic) in provider_names {
        let owner = hir
            .module
            .symbols
            .get(&symbol_name)
            .and_then(|symbols| symbols.value_def)
            .ok_or_else(|| {
                CompilerError::message(format!("hosted provider `{symbol_name}` has no DefId"))
            })?;
        provider_owners.insert(owner, intrinsic);
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
    let mut fir = lower_fir(&bodies, &typed);
    if !fir.diagnostics.is_empty() {
        return Err(CompilerError::message(format!(
            "FIR lowering failed: {:?}",
            fir.diagnostics
        )));
    }

    rewrite_string_comparisons(&mut fir.module, &provider_owners)?;
    let static_initializers = materialize_string_literals(&mut fir.module)?;

    let main_owner = hir
        .module
        .symbols
        .get(entry_name)
        .and_then(|symbols| symbols.value_def)
        .ok_or_else(|| {
            CompilerError::message(format!(
                "native executable requires entry function `{entry_name}`"
            ))
        })?;
    let main = fir.module.functions.get(&main_owner).ok_or_else(|| {
        CompilerError::message(format!("entry `{entry_name}` did not lower to FIR"))
    })?;
    if !main.params.is_empty()
        || main.return_type
            != (Ty::Int {
                signed: true,
                width: IntWidth::W32,
            })
    {
        return Err(CompilerError::message(format!(
            "C14 entry point `{entry_name}` must have signature fn {entry_name}() -> i32"
        )));
    }

    let backend = CraneliftBackend::aarch64()?;
    let prepared = backend.prepare_module_with_static_initializers(
        &fir.module,
        &definitions,
        &static_initializers,
    )?;
    let initializer_owner = prepared.module_initializer_owner();
    let mut exports = vec![main_owner];
    if let Some(owner) = initializer_owner {
        exports.push(owner);
    }
    let imports = reachable_provider_owners(&fir.module, main_owner, &provider_owners);
    let plan = backend.plan_object_module_with_export_names_and_imports(
        &prepared,
        exports,
        external_entry_symbol.map(|symbol| (main_owner, symbol.to_owned())),
        imports.iter().copied(),
    )?;
    let main_symbol = plan
        .symbol(main_owner)
        .ok_or_else(|| CompilerError::message("object plan omitted selected entry"))?
        .name()
        .to_owned();
    let initializer_symbol = initializer_owner
        .map(|owner| {
            plan.symbol(owner)
                .map(|symbol| symbol.name().to_owned())
                .ok_or_else(|| CompilerError::message("object plan omitted module initializer"))
        })
        .transpose()?;
    let mut hosted_providers = BTreeMap::new();
    for owner in &imports {
        let intrinsic = provider_owners
            .get(owner)
            .expect("reachable provider must have an intrinsic name");
        let symbol = plan
            .symbol(*owner)
            .ok_or_else(|| CompilerError::message("object plan omitted hosted provider"))?
            .name()
            .to_owned();
        hosted_providers.insert(intrinsic.clone(), symbol);
    }
    let console_symbol = hosted_providers.get("__forge_console_write").cloned();
    let object = backend.emit_object(&prepared, &plan)?.into_bytes();

    Ok(CompiledProgram {
        object,
        main_owner,
        main_symbol,
        initializer_symbol,
        console_symbol,
        hosted_providers,
    })
}

fn provider_intrinsic_name(symbol: &str) -> Option<&str> {
    if let Some(index) = symbol.rfind("____forge_") {
        return Some(&symbol[index + 2..]);
    }
    if symbol.starts_with("__forge_c12c_") {
        return None;
    }
    symbol.starts_with("__forge_").then_some(symbol)
}

fn rewrite_string_comparisons(
    module: &mut FirModule,
    provider_owners: &BTreeMap<DefId, String>,
) -> Result<(), CompilerError> {
    let find_provider = |name: &str| {
        provider_owners
            .iter()
            .find_map(|(owner, intrinsic)| (intrinsic == name).then_some(*owner))
    };
    let equal = find_provider("__forge_string_equal");
    let not_equal = find_provider("__forge_string_not_equal");

    fn rewrite(
        function: &mut FirFunction,
        equal: Option<DefId>,
        not_equal: Option<DefId>,
    ) -> Result<(), CompilerError> {
        let types = function.value_types.clone();
        for block in &mut function.blocks {
            for instruction in &mut block.instructions {
                let replacement = match &instruction.kind {
                    FirInstructionKind::Binary {
                        op, left, right, ..
                    } if types.get(left) == Some(&Ty::Str)
                        && types.get(right) == Some(&Ty::Str) =>
                    {
                        let target = match op {
                            BinaryOp::Eq => equal,
                            BinaryOp::NotEq => not_equal,
                            _ => None,
                        };
                        target.map(|target| (target, *left, *right))
                    }
                    _ => None,
                };
                if let Some((target, left, right)) = replacement {
                    instruction.kind = FirInstructionKind::Call {
                        target,
                        args: vec![left, right],
                        tail: false,
                    };
                } else if matches!(
                    &instruction.kind,
                    FirInstructionKind::Binary { op: BinaryOp::Eq | BinaryOp::NotEq, left, right, .. }
                        if types.get(left) == Some(&Ty::Str) && types.get(right) == Some(&Ty::Str)
                ) {
                    return Err(CompilerError::message(
                        "native str equality requires the std.string hosted provider",
                    ));
                }
            }
        }
        Ok(())
    }

    for function in module.functions.values_mut() {
        rewrite(function, equal, not_equal)?;
    }
    for initializer in module.global_initializers.values_mut() {
        rewrite(&mut initializer.function, equal, not_equal)?;
    }
    Ok(())
}

fn reachable_provider_owners(
    module: &FirModule,
    main_owner: DefId,
    provider_owners: &BTreeMap<DefId, String>,
) -> BTreeSet<DefId> {
    fn calls(function: &FirFunction) -> Vec<DefId> {
        function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| match instruction.kind {
                FirInstructionKind::Call { target, .. } => Some(target),
                FirInstructionKind::CollectionPatternLookup { operation, .. }
                | FirInstructionKind::CollectionPatternHasOnly { operation, .. } => Some(operation),
                _ => None,
            })
            .collect()
    }

    let mut pending = vec![main_owner];
    for initializer in module.global_initializers.values() {
        pending.extend(calls(&initializer.function));
    }
    let mut visited = BTreeSet::new();
    let mut providers = BTreeSet::new();
    while let Some(owner) = pending.pop() {
        if !visited.insert(owner) {
            continue;
        }
        if provider_owners.contains_key(&owner) {
            providers.insert(owner);
            continue;
        }
        if let Some(function) = module.functions.get(&owner) {
            pending.extend(calls(function));
        }
    }
    providers
}

fn materialize_string_literals(
    module: &mut FirModule,
) -> Result<StaticGlobalInitializerTable, CompilerError> {
    fn collect(function: &FirFunction, literals: &mut BTreeSet<String>) {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let FirInstructionKind::Const {
                    value: FirConst::String { value },
                } = &instruction.kind
                {
                    literals.insert(value.clone());
                }
            }
        }
    }

    fn allocate(used: &mut BTreeSet<DefId>, next: &mut u32) -> Result<DefId, CompilerError> {
        loop {
            let candidate = DefId(*next);
            if used.insert(candidate) {
                if *next > 0 {
                    *next -= 1;
                }
                return Ok(candidate);
            }
            if *next == 0 {
                return Err(CompilerError::message(
                    "no DefId remains for native string literal storage",
                ));
            }
            *next -= 1;
        }
    }

    let mut literals = BTreeSet::new();
    for function in module.functions.values() {
        collect(function, &mut literals);
    }
    for initializer in module.global_initializers.values() {
        collect(&initializer.function, &mut literals);
    }

    let mut used = BTreeSet::new();
    used.extend(module.functions.keys().copied());
    used.extend(module.globals.keys().copied());
    let mut next = u32::MAX;
    let mut descriptors = BTreeMap::new();
    let mut static_initializers = StaticGlobalInitializerTable::new();

    for literal in literals {
        let bytes_owner = allocate(&mut used, &mut next)?;
        let descriptor_owner = allocate(&mut used, &mut next)?;
        let bytes = literal.as_bytes();
        let stored_bytes = if bytes.is_empty() {
            vec![0_u8]
        } else {
            bytes.to_vec()
        };
        let bytes_ty = Ty::Array {
            element: Box::new(Ty::Byte),
            length: Some(stored_bytes.len() as u64),
        };
        module.globals.insert(
            bytes_owner,
            FirGlobal {
                owner: bytes_owner,
                ty: bytes_ty,
                mutable: false,
                constant: None,
            },
        );
        static_initializers.insert(
            bytes_owner,
            StaticGlobalInitializer {
                value: StaticValue::Array(
                    stored_bytes
                        .iter()
                        .map(|byte| {
                            StaticValue::Scalar(ConstValue::Integer {
                                value: i128::from(*byte),
                            })
                        })
                        .collect(),
                ),
                writable: false,
            },
        );

        module.globals.insert(
            descriptor_owner,
            FirGlobal {
                owner: descriptor_owner,
                ty: Ty::Str,
                mutable: false,
                constant: None,
            },
        );
        let mut fields = BTreeMap::new();
        fields.insert(
            "data".to_owned(),
            StaticValue::Address {
                target: StaticSymbol::Global(bytes_owner),
                addend: 0,
            },
        );
        fields.insert(
            "len".to_owned(),
            StaticValue::Scalar(ConstValue::Integer {
                value: bytes.len() as i128,
            }),
        );
        static_initializers.insert(
            descriptor_owner,
            StaticGlobalInitializer {
                value: StaticValue::Aggregate {
                    variant: None,
                    fields,
                },
                writable: false,
            },
        );
        descriptors.insert(literal, descriptor_owner);
    }

    fn rewrite(function: &mut FirFunction, descriptors: &BTreeMap<String, DefId>) {
        for block in &mut function.blocks {
            for instruction in &mut block.instructions {
                let descriptor = match &instruction.kind {
                    FirInstructionKind::Const {
                        value: FirConst::String { value },
                    } => descriptors.get(value).copied(),
                    _ => None,
                };
                if let Some(global) = descriptor {
                    instruction.kind = FirInstructionKind::LoadGlobal { global };
                }
            }
        }
    }
    for function in module.functions.values_mut() {
        rewrite(function, &descriptors);
    }
    for initializer in module.global_initializers.values_mut() {
        rewrite(&mut initializer.function, &descriptors);
    }

    Ok(static_initializers)
}

fn read_library_sources(
    libraries: &[LibraryInput],
) -> Result<Vec<(String, String)>, CompilerError> {
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
    Ok(sources)
}

fn read_source(path: &Path) -> Result<String, CompilerError> {
    fs::read_to_string(path)
        .map_err(|error| CompilerError::message(format!("cannot read {}: {error}", path.display())))
}

pub fn compile_file(path: &Path) -> Result<CompiledProgram, CompilerError> {
    compile_file_with_libraries(path, &[])
}

pub fn compile_file_with_libraries(
    path: &Path,
    libraries: &[LibraryInput],
) -> Result<CompiledProgram, CompilerError> {
    compile_file_with_libraries_for_entry(path, libraries, "main", None)
}

pub fn compile_file_with_libraries_and_entry(
    path: &Path,
    libraries: &[LibraryInput],
    entry: &str,
) -> Result<CompiledProgram, CompilerError> {
    compile_file_with_libraries_for_entry(path, libraries, entry, Some(entry))
}

fn compile_file_with_libraries_for_entry(
    path: &Path,
    libraries: &[LibraryInput],
    entry: &str,
    external_entry_symbol: Option<&str>,
) -> Result<CompiledProgram, CompilerError> {
    let source = read_source(path)?;
    let sources = read_library_sources(libraries)?;
    compile_source_with_library_sources_for_entry(&source, &sources, entry, external_entry_symbol)
}

pub fn check_file(path: &Path) -> Result<(), CompilerError> {
    check_file_with_libraries(path, &[])
}

pub fn check_file_with_libraries(
    path: &Path,
    libraries: &[LibraryInput],
) -> Result<(), CompilerError> {
    let mut source = read_source(path)?;
    source.push_str("\nfn ");
    source.push_str(CHECK_ENTRY);
    source.push_str("() -> i32 { return 0; }\n");
    let sources = read_library_sources(libraries)?;
    compile_source_with_library_sources_for_entry(&source, &sources, CHECK_ENTRY, None).map(|_| ())
}

pub fn emit_object_file(path: &Path, output: &Path) -> Result<(), CompilerError> {
    emit_object_file_with_libraries(path, output, &[])
}

pub fn emit_object_file_with_libraries(
    path: &Path,
    output: &Path,
    libraries: &[LibraryInput],
) -> Result<(), CompilerError> {
    emit_object_file_with_libraries_and_entry(path, output, libraries, "main")
}

pub fn emit_object_file_with_libraries_and_entry(
    path: &Path,
    output: &Path,
    libraries: &[LibraryInput],
    entry: &str,
) -> Result<(), CompilerError> {
    let compiled = compile_file_with_libraries_and_entry(path, libraries, entry)?;
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
    build_executable_with_libraries_and_entry(path, output, libraries, "main")
}

pub fn build_executable_with_libraries_and_entry(
    path: &Path,
    output: &Path,
    libraries: &[LibraryInput],
    entry: &str,
) -> Result<(), CompilerError> {
    require_native_aarch64_linux()?;
    let compiled = compile_file_with_libraries_and_entry(path, libraries, entry)?;
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
    run_file_with_libraries_and_entry(path, args, libraries, "main")
}

pub fn run_file_with_libraries_and_entry(
    path: &Path,
    args: &[String],
    libraries: &[LibraryInput],
    entry: &str,
) -> Result<Output, CompilerError> {
    require_native_aarch64_linux()?;
    let dir = temporary_directory("run")?;
    let executable = dir.join("program");
    let result = (|| {
        let compiled = compile_file_with_libraries_and_entry(path, libraries, entry)?;
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
    let hosted_providers = hosted_provider_source(program.hosted_provider_symbols());
    let initializer = program
        .initializer_symbol()
        .map_or_else(String::new, |symbol| {
            format!("extern void {symbol}(void);\n")
        });
    let initialize = program
        .initializer_symbol()
        .map_or_else(String::new, |symbol| format!("    {symbol}();\n"));
    format!(
        "#include <stdint.h>\n#include <stdio.h>\n#include <stdlib.h>\n\n\
         extern int32_t {main_symbol}(void);\n\
         {initializer}\n\
         {hosted_providers}\n\
         __attribute__((noreturn)) void __forge_panic(const void *info) {{\n\
             (void)info;\n\
             abort();\n\
         }}\n\n\
         int main(void) {{\n\
         {initialize}\n\
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
            "C14 hosted linking/execution currently requires native AArch64 Linux",
        ))
    }
}

fn temporary_directory(label: &str) -> Result<PathBuf, CompilerError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let serial = NEXT.fetch_add(1, Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("forge-c14-{label}-{}-{serial}", std::process::id()));
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

    const CUSTOM_ENTRY: &str = r#"
module test.custom_entry;
fn start() -> i32 {
    return 0;
}
"#;

    const KERNEL_OBJECT: &str = r#"
module test.kernel_object;
fn helper() -> i32 {
    return 0;
}
fn start() -> i32 {
    return helper();
}
"#;

    const LIBRARY_ONLY: &str = r#"
module test.library_only;
pub fn forty_two() -> i32 { return 42; }
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
    fn custom_entry_compiles_without_main() {
        let compiled = compile_source_with_library_sources_and_entry(CUSTOM_ENTRY, &[], "start")
            .expect("compile custom entry");
        assert_eq!(&compiled.object()[..4], b"\x7fELF");
        assert_eq!(compiled.main_symbol(), "start");
    }

    #[test]
    fn freestanding_entry_object_has_explicit_elf_contract_without_hosted_imports() {
        let compiled = compile_source_with_library_sources_and_entry(
            KERNEL_OBJECT,
            &[("core".into(), include_str!("../../../lib/core.fg").into())],
            "start",
        )
        .expect("compile freestanding kernel object");

        assert_eq!(compiled.main_symbol(), "start");
        assert!(compiled.hosted_provider_symbols().is_empty());

        let sections = elf64_sections(compiled.object());
        for name in [
            ".text",
            ".rela.text",
            ".rodata",
            ".rela.rodata",
            ".data",
            ".rela.data",
            ".bss",
            ".symtab",
            ".strtab",
            ".note.GNU-stack",
        ] {
            assert!(
                sections.iter().any(|section| section.name == name),
                "missing kernel object section {name}: {sections:#?}"
            );
        }
        let text = sections
            .iter()
            .find(|section| section.name == ".text")
            .expect("text section");
        assert_eq!(text.kind, 1, ".text must be SHT_PROGBITS");
        assert_eq!(text.flags, 0x6, ".text must be allocatable/executable");
        assert!(text.size > 0, ".text must contain the kernel entry");
        let rela_text = sections
            .iter()
            .find(|section| section.name == ".rela.text")
            .expect("text relocations");
        assert_eq!(rela_text.kind, 4, ".rela.text must be SHT_RELA");
        assert!(rela_text.size > 0, "helper call must retain a relocation");

        let symbols = elf64_symbols(compiled.object(), &sections);
        let entry = symbols
            .iter()
            .find(|symbol| symbol.name == "start")
            .expect("platform-facing start symbol");
        assert_eq!(entry.binding, 1, "entry must be STB_GLOBAL");
        assert_eq!(entry.kind, 2, "entry must be STT_FUNC");
        assert_eq!(usize::from(entry.section), text.index);
        let undefined = symbols
            .iter()
            .filter(|symbol| !symbol.name.is_empty() && symbol.section == 0)
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>();
        assert!(
            undefined.is_empty(),
            "freestanding object must not leak hosted/runtime imports: {undefined:?}"
        );
    }

    #[derive(Debug)]
    struct ElfSection {
        index: usize,
        name: String,
        kind: u32,
        flags: u64,
        offset: usize,
        size: usize,
        link: usize,
        entry_size: usize,
    }

    #[derive(Debug)]
    struct ElfSymbol {
        name: String,
        binding: u8,
        kind: u8,
        section: u16,
    }

    fn elf64_sections(bytes: &[u8]) -> Vec<ElfSection> {
        assert_eq!(&bytes[..4], b"\x7fELF");
        assert_eq!(bytes[4], 2, "kernel object must be ELF64");
        assert_eq!(bytes[5], 1, "kernel object must be little-endian");
        assert_eq!(read_u16(bytes, 16), 1, "kernel object must be relocatable");
        assert_eq!(
            read_u16(bytes, 18),
            183,
            "kernel object must target AArch64"
        );

        let table = read_u64(bytes, 40) as usize;
        let entry_size = usize::from(read_u16(bytes, 58));
        let count = usize::from(read_u16(bytes, 60));
        let names_index = usize::from(read_u16(bytes, 62));
        assert_eq!(entry_size, 64, "unexpected ELF64 section-header size");
        assert!(names_index < count, "invalid section-name string table");

        let raw = (0..count)
            .map(|index| {
                let header = table + index * entry_size;
                (
                    index,
                    read_u32(bytes, header) as usize,
                    read_u32(bytes, header + 4),
                    read_u64(bytes, header + 8),
                    read_u64(bytes, header + 24) as usize,
                    read_u64(bytes, header + 32) as usize,
                    read_u32(bytes, header + 40) as usize,
                    read_u64(bytes, header + 56) as usize,
                )
            })
            .collect::<Vec<_>>();
        let (_, _, _, _, names_offset, names_size, _, _) = raw[names_index];
        let names = slice(bytes, names_offset, names_size);
        raw.into_iter()
            .map(
                |(index, name, kind, flags, offset, size, link, entry_size)| ElfSection {
                    index,
                    name: read_c_string(names, name),
                    kind,
                    flags,
                    offset,
                    size,
                    link,
                    entry_size,
                },
            )
            .collect()
    }

    fn elf64_symbols(bytes: &[u8], sections: &[ElfSection]) -> Vec<ElfSymbol> {
        let symtab = sections
            .iter()
            .find(|section| section.name == ".symtab")
            .expect("symbol table");
        assert_eq!(symtab.kind, 2, ".symtab must be SHT_SYMTAB");
        assert_eq!(symtab.entry_size, 24, "unexpected ELF64 symbol size");
        let strings = sections.get(symtab.link).expect("linked symbol strings");
        let strings = slice(bytes, strings.offset, strings.size);
        let count = symtab.size / symtab.entry_size;
        (0..count)
            .map(|index| {
                let offset = symtab.offset + index * symtab.entry_size;
                let info = bytes[offset + 4];
                ElfSymbol {
                    name: read_c_string(strings, read_u32(bytes, offset) as usize),
                    binding: info >> 4,
                    kind: info & 0x0f,
                    section: read_u16(bytes, offset + 6),
                }
            })
            .collect()
    }

    fn slice(bytes: &[u8], offset: usize, size: usize) -> &[u8] {
        bytes
            .get(offset..offset.checked_add(size).expect("ELF range overflow"))
            .expect("ELF range outside object")
    }

    fn read_c_string(bytes: &[u8], offset: usize) -> String {
        let tail = bytes.get(offset..).expect("ELF string offset");
        let end = tail.iter().position(|byte| *byte == 0).expect("ELF NUL");
        std::str::from_utf8(&tail[..end])
            .expect("ELF UTF-8 symbol/section name")
            .to_owned()
    }

    fn read_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(slice(bytes, offset, 2).try_into().expect("u16 bytes"))
    }

    fn read_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(slice(bytes, offset, 4).try_into().expect("u32 bytes"))
    }

    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        u64::from_le_bytes(slice(bytes, offset, 8).try_into().expect("u64 bytes"))
    }

    #[test]
    fn check_accepts_library_without_main() {
        let dir = temporary_directory("library-check").expect("temp directory");
        let source = dir.join("lib.fg");
        fs::write(&source, LIBRARY_ONLY).expect("write source");
        check_file(&source).expect("check library source");
        let _ = fs::remove_dir_all(dir);
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
            console_symbol: None,
            hosted_providers: BTreeMap::new(),
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
    fn custom_entry_links_and_executes_natively() {
        let dir = temporary_directory("entry-test").expect("temp directory");
        let source = dir.join("entry.fg");
        fs::write(&source, CUSTOM_ENTRY).expect("write source");
        let output = run_file_with_libraries_and_entry(&source, &[], &[], "start")
            .expect("run custom entry");
        assert!(output.status.success(), "status={}", output.status);
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
