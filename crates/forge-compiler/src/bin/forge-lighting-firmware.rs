use std::fs;
use std::path::PathBuf;
use std::process;

use forge_codegen_cranelift::{build_sia32_flat_image, CraneliftBackend, CraneliftTarget, Sia32Object};
use forge_frontend::{
    ast::SourceFile, collect_type_definitions, lower_fir, lower_module, lower_resolved_bodies,
    parse_source, type_check_module, IntWidth, Ty,
};

fn usage() -> ! {
    eprintln!(
        "usage: forge-lighting-firmware <source.fg> [-o firmware.s] [--entry NAME]\n\n\
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

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => output = PathBuf::from(args.next().unwrap_or_else(|| usage())),
            "--entry" => entry = args.next().unwrap_or_else(|| usage()),
            "-h" | "--help" => usage(),
            _ => usage(),
        }
    }

    let source_text = fs::read_to_string(&source)?;
    let ast = parse_clean(&source_text)?;
    let hir = lower_module(&ast);
    if !hir.diagnostics.is_empty() {
        return Err(format!("HIR lowering failed: {:?}", hir.diagnostics).into());
    }
    if !hir.module.imports.is_empty() {
        return Err("firmware bring-up currently requires a single source file with no imports".into());
    }

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
        .get(&entry)
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
    if !function.params.is_empty() || function.return_type != expected_return {
        return Err(format!(
            "firmware entry '{entry}' must have signature fn {entry}() -> i32"
        )
        .into());
    }

    let backend = CraneliftBackend::sia32()?;
    let prepared = backend.prepare_module_with_types(&fir.module, &definitions)?;
    let image = emit_linked_image(&backend, &prepared, &fir.module, owner, &entry)?;
    let assembly = lighting_rom_assembly(&image, &entry);
    fs::write(&output, assembly)?;
    eprintln!(
        "wrote {} bytes of SIA32 Forge code to {} (wrapped as Lighting reset ROM)",
        image.len(),
        output.display()
    );
    Ok(())
}

fn parse_clean(source: &str) -> Result<SourceFile, Box<dyn std::error::Error>> {
    let parsed = parse_source(source);
    if parsed.ast.is_none() || !parsed.diagnostics.is_empty() {
        return Err(format!("parse failed: {:?}", parsed.diagnostics).into());
    }
    Ok(parsed.ast.expect("checked above"))
}

fn emit_linked_image(
    backend: &CraneliftBackend,
    prepared: &forge_codegen_cranelift::PreparedModule,
    module: &forge_fir::FirModule,
    entry: forge_fir::DefId,
    entry_name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut objects = Vec::new();
    let owners = std::iter::once(entry)
        .chain(module.functions.keys().copied().filter(|owner| *owner != entry));
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
    const LIGHTING_ROM_BASE: u32 = 0xffff_0000;
    // The reset shim occupies 0x14 bytes before forge_entry in the ROM source.
    // SIAO32 relocations must use the address where linked Forge text actually
    // executes, not the beginning of the containing ROM.
    const FORGE_TEXT_BASE: u32 = LIGHTING_ROM_BASE + 0x14;
    let image = build_sia32_flat_image(&objects, FORGE_TEXT_BASE, entry_name, 0)?;
    if image.entry() != FORGE_TEXT_BASE {
        return Err(format!(
            "firmware entry must link at Forge text base 0xffff0014, got 0x{:x}",
            image.entry()
        ).into());
    }
    Ok(image.bytes().to_vec())
}

fn lighting_rom_assembly(code: &[u8], entry: &str) -> String {
    let mut out = String::new();
    out.push_str("; Generated by forge-lighting-firmware.\n");
    out.push_str("; Lighting reset ROM wrapper: call Forge, then halt the machine.\n\n");
    out.push_str("reset:\n");
    out.push_str("    ; Firmware owns early-machine stack initialization. Lighting reset leaves GPRs zero.\n");
    out.push_str("    LDPC.W r13, lit_stack_top\n");
    out.push_str("    BL forge_entry\n");
    out.push_str("    LDPC.W r11, lit_halt\n");
    out.push_str("    LI r2, 1\n");
    out.push_str("    SW r2, [r11]\n");
    out.push_str("halted_forever:\n");
    out.push_str("    B halted_forever\n\n");
    out.push_str(".align 4\n");
    out.push_str("lit_halt: .word 0xfff02008\n");
    out.push_str("lit_stack_top: .word 0x01000000\n\n");
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
    out.push_str("\n.romorg 0xf000\n");
    out.push_str("unexpected_trap:\n");
    out.push_str("    B unexpected_trap\n");
    out
}
