use forge_frontend::{
    dump_fir_module, lower_fir, lower_module, lower_resolved_bodies, parse_source,
    type_check_module, verify_fir_boundary, verify_fir_function, verify_fir_module, ConstValue,
    ExprId, FirBlockId, FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace,
    FirTerminator, IntWidth, Ty,
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
fn value_verifier_reports_function_block_instruction_and_value_context() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_value_context;
        fn main() -> u32 { return 7u32; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let (block, instruction_index, value, span) = function
        .blocks
        .iter()
        .flat_map(|block| {
            block
                .instructions
                .iter()
                .enumerate()
                .filter_map(move |(index, instruction)| {
                    instruction
                        .result
                        .map(|value| (block.id, index, value, instruction.span))
                })
        })
        .next()
        .expect("value-producing instruction");
    function.value_types.remove(&value);

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-value")
        .expect("missing-value-type diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic.message.contains(&format!("function {owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains(&format!("value {value:?}")));
}

#[test]
fn module_verifier_rejects_every_semantic_sentinel_with_value_context() {
    for sentinel in [
        Ty::Error,
        Ty::Unknown,
        Ty::IntLiteral,
        Ty::FloatLiteral,
        Ty::NoneLiteral,
    ] {
        let (_, _, mut fir) = pipeline(
            r#"
            module test.boundary_semantic_sentinel;
            fn main() -> u32 { return 7u32; }
            "#,
        );
        assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

        let function = fir.module.functions.values_mut().next().unwrap();
        let owner = function.owner;
        let (block, instruction_index, value, span) = function
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .instructions
                    .iter()
                    .enumerate()
                    .filter_map(move |(index, instruction)| {
                        instruction
                            .result
                            .map(|value| (block.id, index, value, instruction.span))
                    })
            })
            .next()
            .expect("value-producing instruction");
        function.value_types.insert(value, sentinel.clone());

        let diagnostic = verify_fir_module(&fir.module)
            .into_iter()
            .find(|diagnostic| diagnostic.code == "fir/verify-type")
            .expect("non-concrete-value-type diagnostic");
        assert_eq!(diagnostic.span, span);
        assert!(diagnostic.message.contains(&format!("function {owner:?}")));
        assert!(diagnostic.message.contains(&format!("block {block:?}")));
        assert!(diagnostic
            .message
            .contains(&format!("instruction {instruction_index}")));
        assert!(diagnostic.message.contains(&format!("value {value:?}")));
        assert!(diagnostic.message.contains(&format!("type {sentinel:?}")));
    }
}

#[test]
fn branch_verifier_reports_function_block_value_and_type_context() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_branch_context;
        fn choose(flag: bool) -> u32 {
            if (flag) { return 1u32; }
            return 2u32;
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let (block, condition) = function
        .blocks
        .iter()
        .find_map(|block| match &block.terminator {
            Some(FirTerminator::Branch { condition, .. }) => Some((block.id, *condition)),
            _ => None,
        })
        .expect("branch terminator");
    function.value_types.remove(&condition);

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-branch")
        .expect("branch-condition diagnostic");
    assert!(diagnostic.message.contains(&format!("function {owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("condition {condition:?}")));
    assert!(diagnostic.message.contains("has type None; expected Bool"));
}

#[test]
fn signature_local_and_closure_verifiers_report_owner_context() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_signature_context;
        fn main(input: u32) -> u32 {
            val factor: u32 = input;
            val scale = [factor](value: u32) -> u32 { return value * factor; };
            return scale(3u32);
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    function.return_type = Ty::Unknown;
    let local_id = *function.locals.keys().next().expect("function local");
    function.locals.get_mut(&local_id).unwrap().ty = Ty::Unknown;

    let closure = function.closures.values_mut().next().expect("closure");
    let closure_id = closure.id;
    closure.entry = FirBlockId(u32::MAX);
    closure.return_type = Ty::Unknown;
    closure.captures[0].ty = Ty::Unknown;

    let diagnostics = verify_fir_module(&fir.module);
    let return_type = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "fir/verify-type" && diagnostic.message.contains("return type")
        })
        .expect("function-return diagnostic");
    assert!(return_type.message.contains(&format!("function {owner:?}")));
    assert!(return_type.message.contains("expected a concrete FIR type"));

    let local = diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.code == "fir/verify-type"
                && diagnostic.message.contains(&format!("local {local_id:?}"))
        })
        .expect("local-type diagnostic");
    assert!(local.message.contains(&format!("function {owner:?}")));

    let entry = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-entry")
        .expect("closure-entry diagnostic");
    assert!(entry.message.contains(&format!("function {owner:?}")));
    assert!(entry.message.contains(&format!("closure {closure_id:?}")));
    assert!(entry.message.contains("entry block FirBlockId(4294967295)"));

    let closure_type = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-type")
        .expect("closure-type diagnostic");
    assert!(closure_type
        .message
        .contains(&format!("function {owner:?}")));
    assert!(closure_type
        .message
        .contains(&format!("closure {closure_id:?}")));
    assert!(closure_type.message.contains("expected concrete FIR types"));
}

#[test]
fn closure_metadata_verifier_rejects_inconsistent_body_identity_and_parameters() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_closure_metadata;
        fn main(input: u32) -> u32 {
            val factor: u32 = input;
            val scale = [factor](value: u32) -> u32 { return value * factor; };
            return scale(3u32);
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let closure_key = *function.closures.keys().next().expect("closure");
    let mut closure = function.closures.remove(&closure_key).expect("closure");
    let parameter = closure.params[0];
    closure.params.push(parameter);
    closure.function_pointer = true;
    let bad_key = ExprId(closure_key.0 + 1);
    function.closures.insert(bad_key, closure);

    let diagnostics = verify_fir_module(&fir.module);
    for code in [
        "fir/verify-closure-id",
        "fir/verify-closure-parameter",
        "fir/verify-closure-captures",
        "fir/verify-closure-owner",
    ] {
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == code)
            .unwrap_or_else(|| panic!("missing {code}: {diagnostics:?}"));
        assert!(diagnostic.message.contains(&format!("function {owner:?}")));
    }
}

#[test]
fn closure_verifier_rejects_control_flow_between_bodies() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_closure_control_flow;
        fn main(input: u32) -> u32 {
            val factor: u32 = input;
            val value = [factor]() -> u32 { return factor; };
            return value();
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let closure_entry = function.closures.values().next().expect("closure").entry;
    let outer = function
        .blocks
        .iter_mut()
        .find(|block| block.closure.is_none())
        .expect("outer block");
    let outer_id = outer.id;
    outer.terminator = Some(FirTerminator::Goto {
        target: closure_entry,
    });

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-control-flow")
        .expect("closure-control-flow diagnostic");
    assert!(diagnostic.message.contains(&format!("function {owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {outer_id:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("targets block {closure_entry:?}")));
}

#[test]
fn function_verifier_rejects_mismatched_block_identity_and_entry_owner() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_block_identity;
        fn main() -> u32 { return 7u32; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let entry = function.entry;
    function.blocks[entry.0 as usize].id = FirBlockId(7);
    function.blocks[entry.0 as usize].closure = Some(ExprId(9));

    let diagnostics = verify_fir_module(&fir.module);
    let block_id = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-block-id")
        .expect("block-id diagnostic");
    assert!(block_id.message.contains(&format!("function {owner:?}")));
    assert!(block_id.message.contains("block vector index 0"));
    assert!(block_id.message.contains("id FirBlockId(7)"));

    let entry_diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-entry")
        .expect("function-entry diagnostic");
    assert!(entry_diagnostic
        .message
        .contains(&format!("function {owner:?}")));
    assert!(entry_diagnostic
        .message
        .contains(&format!("entry block {entry:?}")));
    assert!(entry_diagnostic
        .message
        .contains("expected an existing outer-function block"));
}

#[test]
fn function_verifier_rejects_inconsistent_local_and_parameter_metadata() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_local_metadata;
        fn identity(value: u32) -> u32 { return value; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let parameter = function.params[0];
    function.params.push(parameter);
    function.locals.get_mut(&parameter).unwrap().id = FirLocalId(99);

    let diagnostics = verify_fir_module(&fir.module);
    let local_id = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-local-id")
        .expect("local-id diagnostic");
    assert!(local_id.message.contains(&format!("function {owner:?}")));
    assert!(local_id
        .message
        .contains(&format!("local map key {parameter:?}")));
    assert!(local_id.message.contains("metadata id FirLocalId(99)"));

    let parameter_diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-parameter")
        .expect("function-parameter diagnostic");
    assert!(parameter_diagnostic
        .message
        .contains(&format!("function {owner:?}")));
    assert!(parameter_diagnostic.message.contains("parameter 1"));
    assert!(parameter_diagnostic
        .message
        .contains("expected a unique existing parameter local"));
}

#[test]
fn module_verifier_rejects_inconsistent_callable_owners() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_callable_owners;
        fn seed() -> u32 { return 1u32; }
        val runtime: u32 = seed();
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let (function_key, function) = fir.module.functions.iter_mut().next().expect("function");
    let function_key = *function_key;
    function.owner = forge_frontend::DefId(function_key.0 + 100);

    let (initializer_key, initializer) = fir
        .module
        .global_initializers
        .iter_mut()
        .next()
        .expect("runtime initializer");
    let initializer_key = *initializer_key;
    initializer.function.owner = forge_frontend::DefId(initializer_key.0 + 100);

    let diagnostics = verify_fir_module(&fir.module);
    let function_owner = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-function-owner")
        .expect("function-owner diagnostic");
    assert!(function_owner
        .message
        .contains(&format!("function map key {function_key:?}")));

    let initializer_owner = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-global-init-function-owner")
        .expect("initializer-function-owner diagnostic");
    assert!(initializer_owner
        .message
        .contains(&format!("runtime initializer {initializer_key:?}")));
}

#[test]
fn module_verifier_rejects_parameterized_global_initializers() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_global_initializer_signature;
        fn seed() -> u32 { return 1u32; }
        val runtime: u32 = seed();
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let initializer = fir
        .module
        .global_initializers
        .values_mut()
        .next()
        .expect("runtime initializer");
    let parameter = FirLocalId(0);
    initializer.function.params.push(parameter);
    initializer.function.locals.insert(
        parameter,
        FirLocal {
            id: parameter,
            source: None,
            ty: Ty::Int {
                signed: false,
                width: IntWidth::W32,
            },
            mutable: false,
            parameter: true,
            synthetic: false,
        },
    );

    let diagnostics = verify_fir_module(&fir.module);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-global-init-signature")
        .expect("initializer-signature diagnostic");
    assert!(diagnostic.message.contains("takes 1 parameters"));
    assert!(diagnostic.message.contains("expected none"));
}

#[test]
fn module_verifier_rejects_invalid_global_initialization_metadata() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_global_initialization_metadata;
        const fixed: u32 = 7u32;
        fn seed() -> u32 { return 1u32; }
        val runtime: u32 = seed();
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let constant = fir
        .module
        .globals
        .values_mut()
        .find(|global| global.constant.is_some())
        .expect("compile-time global");
    constant.constant = Some(ConstValue::Bool { value: true });

    let runtime = *fir
        .module
        .global_initializers
        .keys()
        .next()
        .expect("runtime global");
    fir.module.globals.get_mut(&runtime).unwrap().constant = Some(ConstValue::Integer { value: 1 });

    let diagnostics = verify_fir_module(&fir.module);
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fir/verify-global-constant"));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "fir/verify-global-initialization"));
}

#[test]
fn verifier_requires_local_initialization_on_every_incoming_path() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_definite_initialization;
        fn choose(flag: bool) -> u32 {
            var value: u32 = 0u32;
            if (flag) { value = 1u32; } else { value = 2u32; }
            return value;
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let owner = function.owner;
    let local = function
        .locals
        .values()
        .find(|local| local.mutable && !local.parameter)
        .expect("mutable value local")
        .id;
    let entry = function.entry;
    let entry_block = function.blocks.get_mut(entry.0 as usize).unwrap();
    let initial_store = entry_block
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local: stored },
                    ..
                } if *stored == local
            )
        })
        .expect("initial local store");
    entry_block.instructions.remove(initial_store);

    assert!(verify_fir_function(function)
        .iter()
        .all(|diagnostic| diagnostic.code != "fir/verify-initialization"));

    let branch_block = function
        .blocks
        .iter_mut()
        .filter(|block| block.id != entry)
        .find(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.kind,
                    FirInstructionKind::Store {
                        place: FirPlace::Local { local: stored },
                        ..
                    } if *stored == local
                )
            })
        })
        .expect("branch local store");
    let branch_store = branch_block
        .instructions
        .iter()
        .position(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local: stored },
                    ..
                } if *stored == local
            )
        })
        .unwrap();
    branch_block.instructions.remove(branch_store);

    let (load_block, load_index, load_span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Load {
                        place: FirPlace::Local { local: loaded },
                    } if *loaded == local => Some((block.id, index, instruction.span)),
                    _ => None,
                },
            )
        })
        .expect("merged local load");

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-initialization")
        .expect("definite-initialization diagnostic");
    assert_eq!(diagnostic.span, load_span);
    assert!(diagnostic.message.contains(&format!("function {owner:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("block {load_block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {load_index}")));
    assert!(diagnostic.message.contains(&format!("local {local:?}")));
    assert!(diagnostic
        .message
        .contains("every incoming control-flow path"));
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
