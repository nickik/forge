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

#[test]
fn transitive_libraries_compile_in_dependency_order() {
    let root = r#"
module app;
import feature;
fn main() -> i32 { return feature.answer() - 42; }
"#;
    let feature = r#"
module feature;
import math;
pub fn answer() -> i32 { return math.answer(); }
"#;
    let math = r#"
module math;
pub fn answer() -> i32 { return 42; }
"#;

    compile_source_with_library_sources(
        root,
        &[
            ("feature".to_owned(), feature.to_owned()),
            ("math".to_owned(), math.to_owned()),
        ],
    )
    .expect("transitive library graph should compile through object emission");
}

#[test]
fn transitive_library_private_access_is_rejected() {
    let root = r#"
module app;
import feature;
fn main() -> i32 { return feature.answer(); }
"#;
    let feature = r#"
module feature;
import math;
pub fn answer() -> i32 { return math.secret(); }
"#;
    let math = r#"
module math;
fn secret() -> i32 { return 42; }
"#;

    let error = compile_source_with_library_sources(
        root,
        &[
            ("math".to_owned(), math.to_owned()),
            ("feature".to_owned(), feature.to_owned()),
        ],
    )
    .expect_err("private transitive dependency must be rejected");
    assert!(
        error.to_string().contains("private value `math.secret`"),
        "unexpected diagnostic: {error}"
    );
}

#[test]
fn transitive_library_cycle_is_rejected() {
    let root = r#"
module app;
import alpha;
fn main() -> i32 { return alpha.answer(); }
"#;
    let alpha = r#"
module alpha;
import beta;
pub fn answer() -> i32 { return beta.answer(); }
"#;
    let beta = r#"
module beta;
import alpha;
pub fn answer() -> i32 { return alpha.answer(); }
"#;

    let error = compile_source_with_library_sources(
        root,
        &[
            ("beta".to_owned(), beta.to_owned()),
            ("alpha".to_owned(), alpha.to_owned()),
        ],
    )
    .expect_err("library cycle must be rejected");
    assert!(
        error.to_string().contains("library import cycle includes"),
        "unexpected diagnostic: {error}"
    );
}
