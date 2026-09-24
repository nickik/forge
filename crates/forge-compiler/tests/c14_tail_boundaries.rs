use forge_compiler::compile_source;

#[test]
fn required_local_closure_tail_call_is_rejected_before_object_emission() {
    let error = compile_source(
        r#"
        module test.closure_tail_boundary;
        fn main() -> i32 {
            val offset: i32 = 7;
            val add = [offset](value: i32) -> i32 { return value + offset; };
            return tail add(5);
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
