use forge_frontend::{ast::DeclKind, parse_source};
use std::{fs, path::PathBuf};

fn collections_source() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/forge-collections-native/src/lib.fg");
    fs::read_to_string(path).expect("read generated native collections")
}

#[test]
fn generated_native_collections_parse() {
    let source = collections_source();
    let parsed = parse_source(&source);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
}

#[test]
fn list_u64_has_no_allocator_field() {
    let parsed = parse_source(&collections_source());
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let file = parsed.ast.expect("collections AST");

    let list = file
        .declarations
        .iter()
        .find_map(|decl| match &decl.kind.kind {
            DeclKind::Struct(x) if x.name == "ListU64" => Some(x),
            _ => None,
        })
        .expect("ListU64 struct");

    let fields: Vec<_> = list.fields.iter().map(|field| field.name.as_str()).collect();
    assert_eq!(fields, ["block", "len", "capacity"]);
}

#[test]
fn list_u64_policy_is_forge_source() {
    let source = collections_source();

    for function in [
        "list_u64_create",
        "list_u64_with_capacity",
        "list_u64_try_reserve",
        "list_u64_push",
        "list_u64_insert",
        "list_u64_pop",
        "list_u64_get",
        "list_u64_set",
        "list_u64_remove",
        "list_u64_swap_remove",
        "list_u64_clear",
        "list_u64_truncate",
        "list_u64_destroy",
    ] {
        assert!(
            source.contains(&format!("pub fn {function}")),
            "{function} is not Forge code"
        );
        assert!(
            !source.contains(&format!("nfn {function}")),
            "{function} leaked into backend ABI"
        );
    }

    assert!(source.contains("core.allocator_alloc(allocator, request)?"));
    assert!(source.contains("core.allocator_resize(allocator, old_block, replacement_bytes)?"));
    assert!(source.contains("core.allocator_free(allocator, block)?;"));
}

#[test]
fn list_u64_commits_resize_only_after_success() {
    let source = collections_source();
    let reserve_start = source.find("pub fn list_u64_try_reserve").expect("reserve start");
    let reserve_end = source[reserve_start..]
        .find("pub fn list_u64_push")
        .map(|offset| reserve_start + offset)
        .expect("reserve end");
    let reserve = &source[reserve_start..reserve_end];

    let resize = reserve
        .find("core.allocator_resize(allocator, old_block, replacement_bytes)?")
        .expect("fallible resize");
    let resize_tail = &reserve[resize..];
    let commit_block = resize_tail
        .find("list.block = memory_block_some(replacement);")
        .expect("block commit after resize");
    let commit_capacity = resize_tail
        .find("list.capacity = replacement_capacity;")
        .expect("capacity commit after resize");

    assert!(commit_block < commit_capacity);
}

#[test]
fn list_u64_destroy_resets_only_after_successful_free() {
    let source = collections_source();
    let destroy_start = source.find("pub fn list_u64_destroy").expect("destroy start");
    let destroy_end = source[destroy_start..]
        .find("// HashSetU64")
        .map(|offset| destroy_start + offset)
        .expect("destroy end");
    let destroy = &source[destroy_start..destroy_end];

    let free = destroy
        .find("core.allocator_free(allocator, block)?;")
        .expect("fallible free");
    let reset = destroy
        .find("list.block = memory_block_none();")
        .expect("block reset");

    assert!(free < reset);
}
