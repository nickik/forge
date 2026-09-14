use forge_frontend::{
    ast::DeclKind,
    parse_source,
};
use std::{collections::BTreeSet, fs, path::PathBuf};

fn core_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../lib/core.fg");
    fs::read_to_string(path).expect("read shipped lib/core.fg")
}

#[test]
fn shipped_core_library_parses_freestanding() {
    let source = core_source();
    let parsed = parse_source(&source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);

    let file = parsed.ast.expect("core AST");
    assert_eq!(file.module.segments, ["core"]);
    assert!(file.imports.is_empty(), "core must not import hosted libraries");
}

#[test]
fn core_exports_required_bootstrap_contracts() {
    let parsed = parse_source(&core_source());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = parsed.ast.expect("core AST");

    let mut exported = BTreeSet::new();
    for decl in &file.declarations {
        match &decl.kind {
            DeclKind::Function(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::Struct(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::Enum(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::Tagged(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::Distinct(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::TypeAlias(x) if x.public => { exported.insert(x.name.as_str()); }
            DeclKind::Global { public: true, value } => { exported.insert(value.name.as_str()); }
            _ => {}
        }
    }

    for required in [
        "PanicKind",
        "PanicLocation",
        "PanicInfo",
        "AllocError",
        "AllocWait",
        "AllocRequest",
        "MemoryBlock",
        "ObjectCacheSpec",
        "min_i32",
        "max_i32",
        "min_u32",
        "max_u32",
        "is_power_of_two_usize",
        "valid_alignment",
    ] {
        assert!(exported.contains(required), "missing public core declaration {required}");
    }
}

#[test]
fn panic_info_is_allocation_free_data() {
    let parsed = parse_source(&core_source());
    let file = parsed.ast.expect("core AST");

    let panic_info = file.declarations.iter().find_map(|decl| match &decl.kind {
        DeclKind::Struct(x) if x.name == "PanicInfo" => Some(x),
        _ => None,
    }).expect("PanicInfo struct");

    let names: Vec<_> = panic_info.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["kind", "message", "location"]);
}

#[test]
fn object_cache_spec_is_explicit_fixed_size_contract() {
    let parsed = parse_source(&core_source());
    let file = parsed.ast.expect("core AST");

    let spec = file.declarations.iter().find_map(|decl| match &decl.kind {
        DeclKind::Struct(x) if x.name == "ObjectCacheSpec" => Some(x),
        _ => None,
    }).expect("ObjectCacheSpec struct");

    let names: Vec<_> = spec.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["object_size", "object_align", "slab_size"]);
}

#[test]
fn variable_sized_allocation_is_part_of_core_contract() {
    let parsed = parse_source(&core_source());
    let file = parsed.ast.expect("core AST");

    let block = file.declarations.iter().find_map(|decl| match &decl.kind {
        DeclKind::Struct(x) if x.name == "MemoryBlock" => Some(x),
        _ => None,
    }).expect("MemoryBlock struct");

    let names: Vec<_> = block.fields.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["data", "size", "align"]);
}
