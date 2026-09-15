use forge_frontend::{
    dump_fir_module, lower_fir, lower_module, lower_resolved_bodies, parse_source,
    type_check_module, verify_fir_boundary, verify_fir_module, FirInstructionKind, FirModule, Ty,
};

fn pipeline(
    source: &str,
) -> (
    forge_frontend::BodyHirOutput,
    forge_frontend::TypeCheckOutput,
    forge_frontend::FirOutput,
) {
    let parsed = parse_source(source);
    assert!(
        parsed.diagnostics.is_empty(),
        "parse: {:?}",
        parsed.diagnostics
    );
    let ast = parsed.ast.expect("AST");
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "hir: {:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(
        bodies.diagnostics.is_empty(),
        "body HIR: {:?}",
        bodies.diagnostics
    );
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(
        typed.diagnostics.is_empty(),
        "typed: {:?}",
        typed.diagnostics
    );
    let fir = lower_fir(&bodies, &typed);
    (bodies, typed, fir)
}

#[test]
fn successful_semantics_cross_a_clean_boundary() {
    let (bodies, typed, fir) = pipeline(
        r#"
        module test.boundary_clean;
        fn choose(flag: bool) -> u32 {
            return match (flag) { true => 1u32, false => 2u32, };
        }
        "#,
    );
    assert!(verify_fir_boundary(&bodies, &typed).is_empty());
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    assert!(verify_fir_module(&fir.module).is_empty());
}

#[test]
fn boundary_rejects_non_concrete_typed_expression() {
    let parsed = parse_source(
        r#"
        module test.boundary_bad_type;
        fn main() -> u32 { return 1u32; }
        "#,
    );
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let mut typed = type_check_module(&ast, &hir.module, &bodies);
    typed.functions.values_mut().next().unwrap().expressions[0].ty = Ty::Unknown;
    let diagnostics = verify_fir_boundary(&bodies, &typed);
    assert!(diagnostics.iter().any(|d| d.code == "fir/boundary-type"));
}

#[test]
fn module_verifier_rejects_poison_and_bad_initializer_order() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_global_init;
        fn seed() -> u32 { return 1u32; }
        val dependent: u32 = base + 1u32;
        val base: u32 = seed();
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    fir.module.global_init_order.reverse();
    let function = fir.module.functions.values_mut().next().unwrap();
    let instruction = function
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .next()
        .expect("function instruction");
    instruction.kind = FirInstructionKind::Poison;

    let diagnostics = verify_fir_module(&fir.module);
    assert!(diagnostics.iter().any(|d| d.code == "fir/verify-poison"));
    assert!(diagnostics
        .iter()
        .any(|d| d.code == "fir/verify-global-init-order"));
}

#[test]
fn duration_reader_form_is_semantically_closed_at_boundary() {
    let (bodies, typed, fir) = pipeline(
        r#"
        module test.boundary_duration;
        struct Jobs {}
        impl Jobs { fn recv(self: &Jobs) -> u32 { return 1u32; } }
        fn main(jobs: Jobs) -> void {
            select {
                recv jobs -> _ => { return; }
                timeout #duration "5ms" => { return; }
            }
        }
        "#,
    );
    assert!(verify_fir_boundary(&bodies, &typed).is_empty());
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
}

#[test]
fn empty_module_dump_matches_checked_in_golden() {
    let actual = dump_fir_module(&FirModule::default()) + "\n";
    let expected = include_str!("golden/fir_module_empty_v1.json");
    assert_eq!(actual, expected);
}
