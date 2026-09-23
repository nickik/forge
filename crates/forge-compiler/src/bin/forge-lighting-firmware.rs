use std::fs;
use std::path::PathBuf;
use std::process;

use forge_codegen_cranelift::{
    build_sia32_flat_image, CraneliftBackend, CraneliftTarget, Sia32Object,
};
use forge_compiler::{link_source_with_library_sources, LibraryInput};
use forge_frontend::{
    ast::SourceFile, collect_type_definitions, lower_fir, lower_module, lower_resolved_bodies,
    type_check_module, IntWidth, Ty,
};

fn usage() -> ! {
    eprintln!(
        "usage: forge-lighting-firmware <source.fg> [-o firmware.s] [--entry NAME] [--library NAME=PATH]... [--raw-image | --user-image] [--text-base ADDR]\n\n\
         Compiles one relocation-free Forge entry function through the production\n\
         SIA32 Cranelift backend and wraps it as Lighting reset-ROM assembly.\n\
         The generated assembly can be turned into a ROM blob with LightingSimulation's siaasm."
    );
    process::exit(64);
}

fn main() {
    if let Err(error) = real_main() {
        eprintln!("forge-lighting-firmware: {error}");
        process::exit(1);
    }
}

fn real_main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let source = PathBuf::from(args.next().unwrap_or_else(|| usage()));
    let mut output = source.with_extension("lighting.s");
    let mut entry = "main".to_owned();
    let mut text_base: u32 = 0xffff_0014;
    let mut raw_image = false;
    let mut payload: Option<(PathBuf, u32)> = None;
    let mut user_image = false;
    let mut library_specs = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => output = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--entry" => entry = args.next().unwrap_or_else(|| usage()),
            "--text-base" => {
                let value = args.next().unwrap_or_else(|| usage());
                text_base = if let Some(hex) = value.strip_prefix("0x") {
                    u32::from_str_radix(hex, 16)?
                } else {
                    value.parse()?
                };
            }
            "--raw-image" => raw_image = true,
            "--user-image" => {
                user_image = true;
                raw_image = true;
            }
            "--library" => library_specs.push(args.next().unwrap_or_else(|| usage())),
            "--embed-payload" => {
                let path = PathBuf::from(args.next().unwrap_or_else(|| usage()));
                let off = args.next().unwrap_or_else(|| usage());
                let offset = if let Some(hex) = off.strip_prefix("0x") {
                    u32::from_str_radix(hex, 16)?
                } else {
                    off.parse()?
                };
                payload = Some((path, offset));
            }
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }

    if user_image && payload.is_some() {
        return Err("--user-image cannot embed a boot payload".into());
    }
    if user_image && text_base == 0xffff_0014 {
        return Err("--user-image requires an explicit --text-base user virtual address".into());
    }

    let libraries = library_specs
        .iter()
        .map(|spec| LibraryInput::parse(spec))
        .collect::<Result<Vec<_>, _>>()?;
    let source_text = fs::read_to_string(&source)?;
    let library_sources = libraries
        .iter()
        .map(|library| {
            Ok::<_, std::io::Error>((
                library.name().to_owned(),
                fs::read_to_string(library.path())?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ast = link_source_with_library_sources(&source_text, &library_sources)?;
    let image = compile_sia32_image(ast, &entry, raw_image, text_base)?;

    if raw_image {
        fs::write(&output, &image)?;
    } else {
        if text_base != 0xffff_0014 {
            return Err("--text-base requires --raw-image".into());
        }
        let payload_bytes = payload
            .as_ref()
            .map(|(path, off)| Ok::<_, std::io::Error>((fs::read(path)?, *off)))
            .transpose()?;
        let assembly = lighting_rom_assembly(
            &image,
            &entry,
            payload_bytes.as_ref().map(|(b, o)| (b.as_slice(), *o)),
        );
        fs::write(&output, assembly)?;
    }
    eprintln!(
        "wrote {} bytes of SIA32 Forge {} to {}",
        image.len(),
        if user_image {
            "user image"
        } else if raw_image {
            "raw image"
        } else {
            "reset ROM payload"
        },
        output.display()
    );
    Ok(())
}

fn compile_sia32_image(
    ast: SourceFile,
    entry: &str,
    raw_image: bool,
    text_base: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let hir = lower_module(&ast);
    if !hir.diagnostics.is_empty() {
        return Err(format!("HIR lowering failed: {:?}", hir.diagnostics).into());
    }
    debug_assert!(
        hir.module.imports.is_empty(),
        "module linker must consume imports"
    );

    let bodies = lower_resolved_bodies(&ast, &hir.module);
    if !bodies.diagnostics.is_empty() {
        return Err(format!("body HIR lowering failed: {:?}", bodies.diagnostics).into());
    }
    let typed = type_check_module(&ast, &hir.module, &bodies);
    if !typed.diagnostics.is_empty() {
        return Err(format!("type checking failed: {:?}", typed.diagnostics).into());
    }
    let definitions = collect_type_definitions(&ast, &hir.module, &bodies, &typed);
    let fir = lower_fir(&bodies, &typed);
    if !fir.diagnostics.is_empty() {
        return Err(format!("FIR lowering failed: {:?}", fir.diagnostics).into());
    }

    let owner = hir
        .module
        .symbols
        .get(entry)
        .and_then(|symbols| symbols.value_def)
        .ok_or_else(|| format!("firmware requires entry function '{entry}'"))?;
    let function = fir
        .module
        .functions
        .get(&owner)
        .ok_or_else(|| format!("entry '{entry}' did not lower to FIR"))?;
    let expected_return = Ty::Int {
        signed: true,
        width: IntWidth::W32,
    };
    let valid_entry_params = function.params.is_empty()
        || (raw_image && entry == "m28_trap_entry" && function.params.len() == 1);
    if !valid_entry_params || function.return_type != expected_return {
        let expected = if raw_image && entry == "m28_trap_entry" {
            format!("fn {entry}() -> i32 or fn {entry}(u32) -> i32")
        } else {
            format!("fn {entry}() -> i32")
        };
        return Err(format!("firmware entry '{entry}' must have signature {expected}").into());
    }

    let backend = CraneliftBackend::sia32()?;
    let prepared = backend.prepare_module_with_types(&fir.module, &definitions)?;
    emit_linked_image(&backend, &prepared, &fir.module, owner, entry, text_base)
}

fn emit_linked_image(
    backend: &CraneliftBackend,
    prepared: &forge_codegen_cranelift::PreparedModule,
    module: &forge_fir::FirModule,
    entry: forge_fir::DefId,
    entry_name: &str,
    text_base: u32,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut objects = Vec::new();
    let owners = std::iter::once(entry).chain(
        module
            .functions
            .keys()
            .copied()
            .filter(|owner| *owner != entry),
    );
    for owner in owners {
        let machine = backend.emit_machine_code(prepared, owner)?;
        if machine.target() != CraneliftTarget::Sia32 {
            return Err("compiler selected a non-SIA32 backend".into());
        }
        eprintln!(
            "firmware function DefId({}) size={} first={:02x?} relocations={}",
            owner.0,
            machine.bytes().len(),
            &machine.bytes()[..machine.bytes().len().min(12)],
            machine.relocations().len()
        );
        let mut object = Sia32Object::new(machine.bytes().to_vec());
        object.define_symbol(format!("__forge_fn_{:08x}", owner.0), 0)?;
        if owner == entry {
            object.define_symbol(entry_name, 0)?;
        }
        for relocation in machine.relocations() {
            let addend = i32::try_from(relocation.addend)
                .map_err(|_| "SIA32 relocation addend does not fit i32")?;
            object.add_relocation(
                relocation.offset,
                relocation.kind,
                format!("__forge_fn_{:08x}", relocation.target.0),
                addend,
            )?;
        }
        objects.push(object);
    }

    // The linked bytes execute from ROM_BASE in Lighting, so relocations must
    // contain final architectural addresses rather than zero-based image offsets.
    let image = build_sia32_flat_image(&objects, text_base, entry_name, 0)?;
    if image.entry() != text_base {
        return Err(format!(
            "firmware entry must link at requested text base 0x{text_base:08x}, got 0x{:x}",
            image.entry()
        )
        .into());
    }
    Ok(image.bytes().to_vec())
}

fn lighting_rom_assembly(code: &[u8], entry: &str, payload: Option<(&[u8], u32)>) -> String {
    let mut out = String::new();
    out.push_str("; Generated by forge-lighting-firmware.\n");
    out.push_str("; Lighting reset ROM wrapper: call Forge, then halt the machine.\n\n");
    out.push_str("reset:\n");
    out.push_str("    ; Firmware owns early-machine stack initialization. Lighting reset leaves GPRs zero.\n");
    out.push_str("    LDPC.W r13, lit_stack_top\n");
    out.push_str("    BL forge_entry\n");
    if payload.is_some() {
        out.push_str("    LDPC.W r12, lit_os_entry\n");
        out.push_str("    CALLR r12\n");
    }
    out.push_str("    LDPC.W r11, lit_halt\n");
    out.push_str("    LI r2, 1\n");
    out.push_str("    SW r2, [r11]\n");
    out.push_str("halted_forever:\n");
    out.push_str("    B halted_forever\n\n");
    out.push_str(".align 4\n");
    out.push_str("lit_halt: .word 0xfff02008\n");
    out.push_str("lit_stack_top: .word 0x01000000\n");
    if payload.is_some() {
        out.push_str("lit_os_entry: .word 0x00100000\n");
    }
    out.push('\n');
    out.push_str(&format!("; Forge entry: {entry}\nforge_entry:\n"));
    for chunk in code.chunks(16) {
        out.push_str("    .byte ");
        for (index, byte) in chunk.iter().enumerate() {
            if index != 0 {
                out.push_str(", ");
            }
            out.push_str(&format!("0x{byte:02x}"));
        }
        out.push('\n');
    }
    if let Some((bytes, offset)) = payload {
        out.push_str(&format!("\n.romorg 0x{offset:x}\nforge_payload:\n"));
        for chunk in bytes.chunks(16) {
            out.push_str("    .byte ");
            for (index, byte) in chunk.iter().enumerate() {
                if index != 0 {
                    out.push_str(", ");
                }
                out.push_str(&format!("0x{byte:02x}"));
            }
            out.push('\n');
        }
    }
    out.push_str("\n.romorg 0xf000\n");
    out.push_str("unexpected_trap:\n");
    out.push_str("    B unexpected_trap\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_library_call_emits_a_freestanding_sia32_image() {
        let root = r#"
module test.user;
import support.math;

pub fn system_task_entry() -> i32 {
    return math.answer();
}
"#;
        let library = r#"
module support.math;

pub fn answer() -> i32 {
    return 42;
}
"#;
        let ast = link_source_with_library_sources(
            root,
            &[("support.math".to_owned(), library.to_owned())],
        )
        .expect("semantic module link");
        let image = compile_sia32_image(ast, "system_task_entry", true, 0x0020_0000)
            .expect("SIA32 image emission");

        assert!(!image.is_empty());
    }
}
