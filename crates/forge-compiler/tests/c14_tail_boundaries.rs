use forge_compiler::compile_source;

#[test]
fn required_local_closure_tail_call_is_rejected_before_object_emission() {
    let error = compile_source(
        r#"
        module test.closure_tail_boundary;
        fn main() -> u32 {
            val offset: u32 = 7u32;
            val add = [offset](value: u32) -> u32 { return value + offset; };
            return tail add(5u32);
        }
        "#,
    )
    .expect_err("required closure tail call must remain a backend boundary");

    assert!(
        error
            .to_string()
            .contains("tail closure call is outside Forge v1 C14 requirements"),
        "unexpected diagnostic: {error}"
    );
}
