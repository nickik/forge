use forge_compiler::compile_source_with_library_sources;

#[test]
fn accepts_full_declared_module_name_as_library_key() {
    let library = r#"
module cosmic.kernel.memory.address_space;

pub fn answer() -> i32 {
    return 42;
}
"#;
    let root = r#"
module cosmic.tests.smoke;

import cosmic.kernel.memory.address_space;

fn main() -> i32 {
    return address_space.answer() - 42;
}
"#;
    compile_source_with_library_sources(
        root,
        &[(
            "cosmic.kernel.memory.address_space".to_owned(),
            library.to_owned(),
        )],
    )
    .expect("full module path library key should compile");
}
