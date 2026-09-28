use forge_frontend::{
    collect_type_definitions, dump_fir_module, lower_fir, lower_module, lower_resolved_bodies,
    parse_source, type_check_module, verify_fir_boundary, verify_fir_function, verify_fir_module,
    verify_fir_module_with_types, ConstValue, DefId, ExprId, FirBlockId, FirGlobal,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, IntWidth, Ty,
    TypeDefinitionTable, UnsafeOperationKind,
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

fn pipeline_with_type_definitions(
    source: &str,
) -> (forge_frontend::FirOutput, TypeDefinitionTable) {
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
    let definitions = collect_type_definitions(&ast, &hir.module, &bodies, &typed);
    let fir = lower_fir(&bodies, &typed);
    (fir, definitions)
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
fn module_verifier_rejects_function_global_identity_collisions() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_definition_namespace;
        const fixed: u32 = 7u32;
        fn seed() -> u32 { return 1u32; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function_owner = *fir.module.functions.keys().next().expect("function");
    let global_owner = *fir.module.globals.keys().next().expect("global");
    let mut global = fir.module.globals.remove(&global_owner).unwrap();
    global.owner = function_owner;
    fir.module.globals.insert(function_owner, global);

    let diagnostics = verify_fir_module(&fir.module);
    let diagnostic = diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-definition-namespace")
        .expect("definition-namespace diagnostic");
    assert!(diagnostic.message.contains(&format!("{function_owner:?}")));
    assert!(diagnostic.message.contains("both a function and a global"));
}

#[test]
fn module_verifier_checks_static_data_address_producer_contract() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_static_data_address;
        fn name() -> *byte { return c"name"; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let storage_owner = DefId(u32::MAX);
    let function = fir.module.functions.values_mut().next().expect("function");
    let instruction = function
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Const {
                    value: forge_frontend::FirConst::CString { .. }
                }
            )
        })
        .expect("C-string constant");
    let result = instruction.result.expect("C-string result");
    let span = instruction.span;
    instruction.kind = FirInstructionKind::StaticDataAddress {
        global: storage_owner,
    };
    fir.module.globals.insert(
        storage_owner,
        FirGlobal {
            owner: storage_owner,
            ty: Ty::Array {
                element: Box::new(Ty::Byte),
                length: Some(5),
            },
            mutable: false,
            constant: None,
        },
    );
    assert!(verify_fir_module(&fir.module).is_empty());

    fir.module.globals.get_mut(&storage_owner).unwrap().mutable = true;
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-static-data-address")
        .expect("mutable-storage diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("global {storage_owner:?}")));
    assert!(diagnostic
        .message
        .contains("immutable fixed-length byte-array storage"));

    fir.module.globals.get_mut(&storage_owner).unwrap().mutable = false;
    fir.module
        .functions
        .values_mut()
        .next()
        .unwrap()
        .value_types
        .insert(
            result,
            Ty::Pointer {
                volatile: true,
                inner: Box::new(Ty::Byte),
            },
        );
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-static-data-address")
        .expect("result-type diagnostic");
    assert!(diagnostic.message.contains("volatile: true"));
    assert!(diagnostic.message.contains("non-volatile byte pointer"));
}

#[test]
fn module_verifier_checks_global_access_producer_contracts() {
    let source = r#"
        module test.boundary_global_access;
        var COUNTER: i32 = 1i32;
        fn write(value: &mut i32) { *value = 9i32; }
        fn main() -> i32 {
            COUNTER = 3i32;
            write(&mut COUNTER);
            return COUNTER;
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::LoadGlobal { .. })
                })
            })
        })
        .expect("global-loading function");
    let (block, instruction_index, result, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block
                .instructions
                .iter()
                .enumerate()
                .find_map(|(index, instruction)| {
                    if matches!(&instruction.kind, FirInstructionKind::LoadGlobal { .. }) {
                        Some((
                            block.id,
                            index,
                            instruction.result.expect("load result"),
                            instruction.span,
                        ))
                    } else {
                        None
                    }
                })
        })
        .expect("global load");
    let function_owner = function.owner;
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-global-load")
        .expect("global-load diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains("type Some(Byte)"));
    assert!(diagnostic.message.contains("exact global type"));

    let (_, _, mut fir) = pipeline(source);
    let global = *fir.module.globals.keys().next().expect("global");
    fir.module.globals.get_mut(&global).unwrap().mutable = false;
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-global-store")
        .expect("immutable-global-store diagnostic");
    assert!(diagnostic.message.contains(&format!("global {global:?}")));
    assert!(diagnostic.message.contains("mutable: false"));
    assert!(diagnostic.message.contains("mutable storage"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.kind,
                        FirInstructionKind::AddressOfGlobal { .. }
                    )
                })
            })
        })
        .expect("global-addressing function");
    let result = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| {
            if matches!(
                &instruction.kind,
                FirInstructionKind::AddressOfGlobal { .. }
            ) {
                Some(instruction.result.expect("address result"))
            } else {
                None
            }
        })
        .expect("global address");
    function.value_types.insert(
        result,
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Byte),
        },
    );
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-global-address")
        .expect("global-address diagnostic");
    assert!(diagnostic.message.contains("requested mutable=true"));
    assert!(diagnostic.message.contains("expected result type"));
    assert!(diagnostic.message.contains("inner: Byte"));
}

#[test]
fn module_verifier_checks_dedicated_bitfield_width_contracts() {
    let write_source = r#"
        module test.boundary_bitfield_write;
        bitstruct Status: u16 { ready: 1; mode: 3; reserved: 12; }
        fn update() -> u8 {
            var status: Status = Status(0u16);
            status.mode = 7u8;
            return status.mode;
        }
    "#;

    let (_, _, mut fir) = pipeline(write_source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::BitFieldCheck { .. })
                })
            })
        })
        .expect("bitfield-writing function");
    let function_owner = function.owner;
    let (block, instruction_index, span) = function
        .blocks
        .iter_mut()
        .find_map(|block| {
            block
                .instructions
                .iter_mut()
                .enumerate()
                .find_map(|(index, instruction)| match &mut instruction.kind {
                    FirInstructionKind::BitFieldCheck { width, .. } => {
                        *width = 8;
                        Some((block.id, index, instruction.span))
                    }
                    _ => None,
                })
        })
        .expect("bitfield range check");
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-bitfield-check")
        .expect("bitfield-check diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains("field width 8"));
    assert!(diagnostic.message.contains("Some(8) bits"));

    let (_, _, mut fir) = pipeline(write_source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::BitFieldExtend { .. })
                })
            })
        })
        .expect("bitfield-extending function");
    let source = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::BitFieldExtend { value } => Some(*value),
            _ => None,
        })
        .expect("bitfield extension");
    function.value_types.insert(
        source,
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        },
    );
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-bitfield-extend")
        .expect("bitfield-extend diagnostic");
    assert!(diagnostic.message.contains("Some(32) bits"));
    assert!(diagnostic.message.contains("Some(16) bits"));
    assert!(diagnostic.message.contains("widened to unsigned storage"));

    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_bitfield_read;
        bitstruct Status: u16 { ready: 1; mode: 3; reserved: 12; }
        fn mode(status: Status) -> u8 { return status.mode; }
        "#,
    );
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.kind,
                        FirInstructionKind::BitFieldExtract { .. }
                    )
                })
            })
        })
        .expect("bitfield-extracting function");
    let result = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::BitFieldExtract { .. } => instruction.result,
            _ => None,
        })
        .expect("bitfield extraction result");
    function.value_types.insert(
        result,
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        },
    );
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-bitfield-extract")
        .expect("bitfield-extract diagnostic");
    assert!(diagnostic.message.contains("Some(16) bits"));
    assert!(diagnostic.message.contains("Some(32) bits"));
    assert!(diagnostic.message.contains("without widening"));
}

#[test]
fn module_verifier_checks_function_reference_targets_and_signatures() {
    let source = r#"
        module test.boundary_function_ref;
        fn add_one(value: u32) -> u32 { return value + 1u32; }
        fn main() -> u32 {
            val op: fn(u32) -> u32 = add_one;
            return op(8u32);
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::FunctionRef { .. })
                })
            })
        })
        .expect("function-reference producer");
    let function_owner = function.owner;
    let missing = DefId(u32::MAX);
    let (block, instruction_index, span) = function
        .blocks
        .iter_mut()
        .find_map(|block| {
            block
                .instructions
                .iter_mut()
                .enumerate()
                .find_map(|(index, instruction)| match &mut instruction.kind {
                    FirInstructionKind::FunctionRef { target } => {
                        *target = missing;
                        Some((block.id, index, instruction.span))
                    }
                    _ => None,
                })
        })
        .expect("function reference");
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-function-ref")
        .expect("missing-target diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains(&format!("target {missing:?}")));
    assert!(diagnostic.message.contains("signature None"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::FunctionRef { .. })
                })
            })
        })
        .expect("function-reference producer");
    let result = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::FunctionRef { .. } => instruction.result,
            _ => None,
        })
        .expect("function-reference result");
    function.value_types.insert(
        result,
        Ty::Function {
            params: vec![Ty::Byte],
            result: Box::new(Ty::Int {
                signed: false,
                width: IntWidth::W32,
            }),
            named_arguments: false,
        },
    );
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-function-ref")
        .expect("signature-mismatch diagnostic");
    assert!(diagnostic.message.contains("params: [Byte]"));
    assert!(diagnostic.message.contains("signature Some"));
    assert!(diagnostic.message.contains("exact function result type"));
}

#[test]
fn module_verifier_checks_direct_call_targets_and_signatures() {
    let source = r#"
        module test.boundary_direct_call;
        fn add_one(value: u32) -> u32 { return value + 1u32; }
        fn main() -> u32 { return add_one(8u32); }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function =
        fir.module
            .functions
            .values_mut()
            .find(|function| {
                function.blocks.iter().any(|block| {
                    block.instructions.iter().any(|instruction| {
                        matches!(&instruction.kind, FirInstructionKind::Call { .. })
                    })
                })
            })
            .expect("direct-call producer");
    let function_owner = function.owner;
    let missing = DefId(u32::MAX);
    let (block, instruction_index, span) = function
        .blocks
        .iter_mut()
        .find_map(|block| {
            block
                .instructions
                .iter_mut()
                .enumerate()
                .find_map(|(index, instruction)| match &mut instruction.kind {
                    FirInstructionKind::Call { target, .. } => {
                        *target = missing;
                        Some((block.id, index, instruction.span))
                    }
                    _ => None,
                })
        })
        .expect("direct call");
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-direct-call")
        .expect("missing-target diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains(&format!("call to {missing:?}")));
    assert!(diagnostic.message.contains("signature is None"));

    let (_, _, mut fir) = pipeline(source);
    let function =
        fir.module
            .functions
            .values_mut()
            .find(|function| {
                function.blocks.iter().any(|block| {
                    block.instructions.iter().any(|instruction| {
                        matches!(&instruction.kind, FirInstructionKind::Call { .. })
                    })
                })
            })
            .expect("direct-call producer");
    let argument = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::Call { args, .. } => Some(args[0]),
            _ => None,
        })
        .expect("direct call");
    function.value_types.insert(argument, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-direct-call")
        .expect("argument-type diagnostic");
    assert!(diagnostic.message.contains("types Some([Byte])"));
    assert!(diagnostic.message.contains("target signature is Some"));
    assert!(diagnostic
        .message
        .contains("exact argument and result types"));

    let (_, _, mut fir) = pipeline(source);
    let function =
        fir.module
            .functions
            .values_mut()
            .find(|function| {
                function.blocks.iter().any(|block| {
                    block.instructions.iter().any(|instruction| {
                        matches!(&instruction.kind, FirInstructionKind::Call { .. })
                    })
                })
            })
            .expect("direct-call producer");
    let result = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::Call { .. } => instruction.result,
            _ => None,
        })
        .expect("direct-call result");
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-direct-call")
        .expect("result-type diagnostic");
    assert!(diagnostic.message.contains("with type Some(Byte)"));
    assert!(diagnostic.message.contains("target signature is Some"));
}

#[test]
fn module_verifier_checks_indirect_call_callees_and_signatures() {
    let source = r#"
        module test.boundary_indirect_call;
        fn add_one(value: u32) -> u32 { return value + 1u32; }
        fn main() -> u32 {
            val op: fn(u32) -> u32 = add_one;
            return op(8u32);
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::CallIndirect { .. })
                })
            })
        })
        .expect("indirect-call producer");
    let function_owner = function.owner;
    let (callee, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::CallIndirect { callee, .. } => {
                        Some((*callee, block.id, index, instruction.span))
                    }
                    _ => None,
                },
            )
        })
        .expect("indirect call");
    function.value_types.insert(callee, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-indirect-call")
        .expect("callee-type diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("callee {callee:?} with type Some(Byte)")));
    assert!(diagnostic.message.contains("function-typed callee"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::CallIndirect { .. })
                })
            })
        })
        .expect("indirect-call producer");
    let (argument, result) = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::CallIndirect { args, .. } => {
                Some((args[0], instruction.result.expect("indirect-call result")))
            }
            _ => None,
        })
        .expect("indirect call");
    function.value_types.insert(argument, Ty::Byte);
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-indirect-call")
        .expect("signature diagnostic");
    assert!(diagnostic.message.contains("types Some([Byte])"));
    assert!(diagnostic.message.contains("with type Some(Byte)"));
    assert!(diagnostic
        .message
        .contains("exact argument and result types"));
}

#[test]
fn module_verifier_checks_local_closure_call_contracts() {
    let source = r#"
        module test.boundary_closure_call;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            val scale = [factor](value: u32) -> u32 { return value * factor; };
            return scale(3u32);
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::CallClosure { .. })
                })
            })
        })
        .expect("closure-call producer");
    let function_owner = function.owner;
    let (closure, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::CallClosure { closure, .. } => {
                        Some((*closure, block.id, index, instruction.span))
                    }
                    _ => None,
                },
            )
        })
        .expect("closure call");
    function.value_types.insert(closure, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-call")
        .expect("closure-type diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("callee {closure:?} with type Some(Byte)")));
    assert!(diagnostic.message.contains("closure-typed callee"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::CallClosure { .. })
                })
            })
        })
        .expect("closure-call producer");
    let (argument, result) = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::CallClosure { args, .. } => {
                Some((args[0], instruction.result.expect("closure-call result")))
            }
            _ => None,
        })
        .expect("closure call");
    function.value_types.insert(argument, Ty::Byte);
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-call")
        .expect("signature diagnostic");
    assert!(diagnostic.message.contains("types Some([Byte])"));
    assert!(diagnostic.message.contains("with type Some(Byte)"));
    assert!(diagnostic.message.contains("exact argument/result types"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::CallClosure { .. })
                })
            })
        })
        .expect("closure-call producer");
    function.closures.clear();
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-call")
        .expect("compatible-body diagnostic");
    assert!(diagnostic.message.contains("compatible local body=false"));
    assert!(diagnostic
        .message
        .contains("compatible function-local closure body"));
}

#[test]
fn module_verifier_checks_make_closure_contracts() {
    let source = r#"
        module test.boundary_make_closure;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            val scale = [factor](value: u32) -> u32 { return value * factor; };
            return scale(3u32);
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeClosure { .. })
                })
            })
        })
        .expect("make-closure producer");
    let function_owner = function.owner;
    let (closure, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::MakeClosure { closure, .. } => Some((
                        *closure,
                        instruction.result.expect("make-closure result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("make closure");
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-make-closure")
        .expect("result-type diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("MakeClosure {closure:?}")));
    assert!(diagnostic.message.contains("result Some"));
    assert!(diagnostic.message.contains("with type Some(Byte)"));
    assert!(diagnostic
        .message
        .contains("exact function-local body signature"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeClosure { .. })
                })
            })
        })
        .expect("make-closure producer");
    let capture = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::MakeClosure { captures, .. } => captures.first().copied(),
            _ => None,
        })
        .expect("captured value");
    function.value_types.insert(capture, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-make-closure")
        .expect("capture-type diagnostic");
    assert!(diagnostic.message.contains("with types Some([Byte])"));
    assert!(diagnostic.message.contains("capture environment"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeClosure { .. })
                })
            })
        })
        .expect("make-closure producer");
    function.closures.clear();
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-make-closure")
        .expect("missing-metadata diagnostic");
    assert!(diagnostic.message.contains("has metadata None"));
}

#[test]
fn module_verifier_checks_closure_capture_places() {
    let source = r#"
        module test.boundary_closure_capture;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            val read = [factor]() -> u32 { return factor; };
            return read();
        }
    "#;

    let (_, _, mut fir) = pipeline(source);
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.kind,
                        FirInstructionKind::Load {
                            place: FirPlace::ClosureCapture { .. }
                        }
                    )
                })
            })
        })
        .expect("closure-capture producer");
    let function_owner = function.owner;
    let (closure, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Load {
                        place: FirPlace::ClosureCapture { closure, .. },
                    } => Some((
                        *closure,
                        instruction.result.expect("capture-load result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("closure-capture load");
    function.value_types.insert(result, Ty::Byte);
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-capture")
        .expect("capture type diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("active closure is Some({closure:?})")));
    assert!(diagnostic.message.contains("actual type=Some(Byte)"));
    assert!(diagnostic.message.contains("exact direct access types"));

    let (_, _, mut fir) = pipeline(source);
    let function = fir
        .module
        .functions
        .values_mut()
        .find(|function| {
            function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.kind,
                        FirInstructionKind::Load {
                            place: FirPlace::ClosureCapture { .. }
                        }
                    )
                })
            })
        })
        .expect("closure-capture producer");
    let index = function
        .blocks
        .iter_mut()
        .flat_map(|block| block.instructions.iter_mut())
        .find_map(|instruction| match &mut instruction.kind {
            FirInstructionKind::Load {
                place: FirPlace::ClosureCapture { index, .. },
            } => Some(index),
            _ => None,
        })
        .expect("closure-capture index");
    *index = 99;
    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-closure-capture")
        .expect("capture index diagnostic");
    assert!(diagnostic.message.contains("index: 99"));
    assert!(diagnostic.message.contains("field=None"));
    assert!(diagnostic.message.contains("in-range capture"));
}

#[test]
fn module_verifier_checks_direct_local_load_types() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_local_load;
        fn main(input: u32) -> u32 { return input; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (local, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    } => Some((
                        *local,
                        instruction.result.expect("load result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("direct local load");
    function.value_types.insert(result, Ty::Byte);
    function.return_type = Ty::Byte;

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-local-load")
        .expect("local-load diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("loads local {local:?}")));
    assert!(diagnostic.message.contains("ty: Int"));
    assert!(diagnostic.message.contains("with type Some(Byte)"));
    assert!(diagnostic.message.contains("exact declared local type"));
}

#[test]
fn module_verifier_checks_direct_local_store_types() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_local_store;
        fn main(input: u32) -> u32 {
            var output: u32 = input;
            return output;
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (local, value, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Store {
                        place: FirPlace::Local { local },
                        value,
                    } => Some((*local, *value, block.id, index, instruction.span)),
                    _ => None,
                },
            )
        })
        .expect("direct local store");
    function.locals.get_mut(&local).expect("local").ty = Ty::Byte;

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-local-store")
        .expect("local-store diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("stores value {value:?} with type Some(Int")));
    assert!(diagnostic
        .message
        .contains(&format!("into local {local:?}")));
    assert!(diagnostic.message.contains("ty: Byte"));
    assert!(diagnostic.message.contains("exact declared local type"));
}

#[test]
fn module_verifier_checks_direct_local_address_types_and_mutability() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_local_address;
        fn main(input: u32) -> u32 {
            var value: u32 = input;
            val address: &mut u32 = &mut value;
            return *address;
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (local, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::AddressOf {
                        place: FirPlace::Local { local },
                        mutable: true,
                    } => Some((
                        *local,
                        instruction.result.expect("address result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("direct mutable local address");
    function.locals.get_mut(&local).expect("local").mutable = false;
    function.value_types.insert(
        result,
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Byte),
        },
    );

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-local-address")
        .expect("local-address diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("address of local {local:?}")));
    assert!(diagnostic.message.contains("mutable: false"));
    assert!(diagnostic.message.contains("result type Some(Reference"));
    assert!(diagnostic.message.contains("mutable storage"));
    assert!(diagnostic.message.contains("exact reference type"));
}

#[test]
fn module_verifier_checks_direct_dereference_address_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_dereference_address;
        fn reborrow(address: &mut u32) -> u32 {
            val rebound: &mut u32 = &mut *address;
            return *rebound;
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (address, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::AddressOf {
                        place: FirPlace::Deref { address },
                        mutable: true,
                    } => Some((*address, block.id, index, instruction.span)),
                    _ => None,
                },
            )
        })
        .expect("direct mutable dereference address");
    function.value_types.insert(
        address,
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Int {
                signed: false,
                width: IntWidth::W32,
            }),
        },
    );

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-dereference-address")
        .expect("dereference-address diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("from value {address:?}")));
    assert!(diagnostic.message.contains("mutable: false"));
    assert!(diagnostic.message.contains("mutable source reference"));
    assert!(diagnostic.message.contains("exact reference type"));
}

#[test]
fn module_verifier_checks_direct_safe_dereference_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_safe_dereference;
        fn read(address: &u32) -> u32 { return *address; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (address, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    } => Some((
                        *address,
                        instruction.result.expect("load result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("direct safe dereference load");
    function.value_types.insert(result, Ty::Byte);

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-safe-dereference")
        .expect("safe-dereference diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("reference value {address:?}")));
    assert!(diagnostic.message.contains("actual value type Some(Byte)"));
    assert!(diagnostic.message.contains("exact pointee type"));
}

#[test]
fn module_verifier_checks_direct_raw_dereference_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_raw_dereference;
        fn read(address: *u32) -> u32 { unsafe { return *address; } }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (address, result, block, instruction_index, span) = function
        .blocks
        .iter_mut()
        .find_map(|block| {
            block
                .instructions
                .iter_mut()
                .enumerate()
                .find_map(|(index, instruction)| match &mut instruction.kind {
                    FirInstructionKind::Load {
                        place:
                            FirPlace::RawDeref {
                                address, volatile, ..
                            },
                    } => {
                        *volatile = true;
                        Some((
                            *address,
                            instruction.result.expect("load result"),
                            block.id,
                            index,
                            instruction.span,
                        ))
                    }
                    _ => None,
                })
        })
        .expect("direct raw dereference load");
    function.value_types.insert(result, Ty::Byte);

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-raw-dereference")
        .expect("raw-dereference diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("pointer value {address:?}")));
    assert!(diagnostic.message.contains("place volatility=true"));
    assert!(diagnostic.message.contains("actual value type Some(Byte)"));
    assert!(diagnostic.message.contains("exact pointee type"));
}

#[test]
fn module_verifier_checks_pointer_offset_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_pointer_offset;
        fn next(address: *u32) -> *u32 {
            unsafe { return address + 1usize; }
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (pointer, offset, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::PointerOffset {
                        pointer, offset, ..
                    } => Some((*pointer, *offset, block.id, index, instruction.span)),
                    _ => None,
                },
            )
        })
        .expect("pointer-offset producer");
    function.value_types.insert(offset, Ty::Bool);

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-pointer-offset")
        .expect("pointer-offset diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("pointer value {pointer:?}")));
    assert!(diagnostic.message.contains("with type Some(Bool)"));
    assert!(diagnostic.message.contains("concrete integer offset"));
    assert!(diagnostic
        .message
        .contains("exact base-pointer result type"));
}

#[test]
fn module_verifier_checks_pointer_conversion_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_pointer_convert;
        fn address(value: *u32) -> usize {
            unsafe { return usize(value); }
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (value, block, instruction_index, span) = function
        .blocks
        .iter_mut()
        .find_map(|block| {
            block
                .instructions
                .iter_mut()
                .enumerate()
                .find_map(|(index, instruction)| match &mut instruction.kind {
                    FirInstructionKind::PointerConvert {
                        value, operation, ..
                    } => {
                        *operation = UnsafeOperationKind::PointerOffset { subtract: false };
                        Some((*value, block.id, index, instruction.span))
                    }
                    _ => None,
                })
        })
        .expect("pointer-convert producer");

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-pointer-convert")
        .expect("pointer-convert diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("converts value {value:?}")));
    assert!(diagnostic.message.contains("PointerOffset"));
    assert!(diagnostic.message.contains("operation tag"));
}

#[test]
fn module_verifier_checks_array_construction_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_make_array;
        fn values() -> [u16; 2] { return [1u16, 2u16]; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (item, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::MakeArray { items } => {
                        Some((items[0], block.id, index, instruction.span))
                    }
                    _ => None,
                },
            )
        })
        .expect("make-array producer");
    function.value_types.insert(
        item,
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        },
    );

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-make-array")
        .expect("make-array diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic.message.contains("width: W32"));
    assert!(diagnostic.message.contains("width: W16"));
    assert!(diagnostic.message.contains("exact element type"));
}

#[test]
fn module_verifier_checks_length_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_len;
        fn count(values: u16[]) -> usize { return values.len; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (value, result, block, instruction_index, span) = function
        .blocks
        .iter()
        .find_map(|block| {
            block.instructions.iter().enumerate().find_map(
                |(index, instruction)| match &instruction.kind {
                    FirInstructionKind::Len { value } => Some((
                        *value,
                        instruction.result.expect("length result"),
                        block.id,
                        index,
                        instruction.span,
                    )),
                    _ => None,
                },
            )
        })
        .expect("length producer");
    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    function
        .value_types
        .insert(result, invalid_result_type.clone());
    function.return_type = invalid_result_type;

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-len")
        .expect("length diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("from value {value:?}")));
    assert!(diagnostic.message.contains("width: W32"));
    assert!(diagnostic.message.contains("usize result"));
}

#[test]
fn module_verifier_checks_unchecked_index_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_index;
        fn first(values: u16[]) -> u16 { return values[0]; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let function = fir.module.functions.values_mut().next().unwrap();
    let function_owner = function.owner;
    let (base, index, result, block, instruction_index, span) =
        function
            .blocks
            .iter()
            .find_map(|block| {
                block.instructions.iter().enumerate().find_map(
                    |(instruction_index, instruction)| match &instruction.kind {
                        FirInstructionKind::IndexUnchecked { base, index } => Some((
                            *base,
                            *index,
                            instruction.result.expect("indexed result"),
                            block.id,
                            instruction_index,
                            instruction.span,
                        )),
                        _ => None,
                    },
                )
            })
            .expect("unchecked-index producer");
    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    function
        .value_types
        .insert(result, invalid_result_type.clone());
    function.return_type = invalid_result_type;

    let diagnostic = verify_fir_module(&fir.module)
        .into_iter()
        .find(|diagnostic| diagnostic.code == "fir/verify-index-unchecked")
        .expect("unchecked-index diagnostic");
    assert_eq!(diagnostic.span, span);
    assert!(diagnostic
        .message
        .contains(&format!("function {function_owner:?}")));
    assert!(diagnostic.message.contains(&format!("block {block:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("instruction {instruction_index}")));
    assert!(diagnostic
        .message
        .contains(&format!("indexes base {base:?}")));
    assert!(diagnostic
        .message
        .contains(&format!("using index {index:?}")));
    assert!(diagnostic.message.contains("width: W32"));
    assert!(diagnostic.message.contains("width: W16"));
    assert!(diagnostic.message.contains("exact element result type"));
}

#[test]
fn module_verifier_checks_string_constant_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_string_constants;
        fn name() -> str { return "name"; }
        fn c_name() -> *byte { return c"name"; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    for function in fir.module.functions.values_mut() {
        let result = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match &instruction.kind {
                FirInstructionKind::Const {
                    value:
                        forge_frontend::FirConst::String { .. }
                        | forge_frontend::FirConst::CString { .. },
                } => instruction.result,
                _ => None,
            })
            .expect("string-constant producer");
        function
            .value_types
            .insert(result, invalid_result_type.clone());
        function.return_type = invalid_result_type.clone();
    }

    let diagnostics = verify_fir_module(&fir.module)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-string-constant")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("string constant")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("C-string constant")));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message.contains("width: W32")));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("expected exact result type Str")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("Pointer")));
}

#[test]
fn module_verifier_checks_zero_payload_producer_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_zero_payload;
        fn done() -> Result[void, u8] { return Ok(); }
        fn absent() -> u32? { return None; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    for function in fir.module.functions.values_mut() {
        let (result, change_return_type) = function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .find_map(|instruction| match &instruction.kind {
                FirInstructionKind::Unit => instruction.result.map(|result| (result, false)),
                FirInstructionKind::MakeNone => instruction.result.map(|result| (result, true)),
                _ => None,
            })
            .expect("zero-payload producer");
        function
            .value_types
            .insert(result, invalid_result_type.clone());
        if change_return_type {
            function.return_type = invalid_result_type.clone();
        }
    }

    let diagnostics = verify_fir_module(&fir.module)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-zero-payload-producer")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 2, "{diagnostics:?}");
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("unit result")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("make-none result")));
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message.contains("width: W32")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("exact void result")));
    assert!(diagnostics
        .iter()
        .any(|diagnostic| diagnostic.message.contains("optional result")));
}

#[test]
fn module_verifier_checks_option_operation_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_option_operations;
        fn wrap(value: u16) -> u16? { return Some(value); }
        fn unwrap(value: u16?) -> u16 {
            return match (value) { Some(inner) => inner, _ => 0u16 };
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut changed = 0;
    for function in fir.module.functions.values_mut() {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if matches!(
                    instruction.kind,
                    FirInstructionKind::MakeSome { .. }
                        | FirInstructionKind::OptionIsSome { .. }
                        | FirInstructionKind::OptionUnwrap { .. }
                ) {
                    let result = instruction.result.expect("option-operation result");
                    function
                        .value_types
                        .insert(result, invalid_result_type.clone());
                    changed += 1;
                }
            }
        }
    }
    assert_eq!(changed, 3);

    let diagnostics = verify_fir_module(&fir.module)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-option-operation")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 3, "{diagnostics:?}");
    for operation in ["MakeSome", "OptionIsSome", "OptionUnwrap"] {
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(operation)));
    }
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message.contains("width: W32")));
}

#[test]
fn module_verifier_checks_result_operation_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_result_operations;
        fn ok() -> Result[u32, u8] { return Ok(1u32); }
        fn err() -> Result[u32, u8] { return Err(2u8); }
        fn inspect(value: Result[u32, u8]) -> u32 {
            return match (value) {
                Ok(payload) => payload,
                Err(error) => u32(error),
            };
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W64,
    };
    let mut changed = 0;
    for function in fir.module.functions.values_mut() {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if matches!(
                    instruction.kind,
                    FirInstructionKind::MakeResultOk { .. }
                        | FirInstructionKind::MakeResultErr { .. }
                        | FirInstructionKind::ResultIsOk { .. }
                        | FirInstructionKind::ResultUnwrapOk { .. }
                        | FirInstructionKind::ResultUnwrapErr { .. }
                ) {
                    let result = instruction.result.expect("result-operation result");
                    function
                        .value_types
                        .insert(result, invalid_result_type.clone());
                    changed += 1;
                }
            }
        }
    }
    assert!(changed >= 5);

    let diagnostics = verify_fir_module(&fir.module)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-result-operation")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), changed, "{diagnostics:?}");
    for operation in [
        "MakeResultOk",
        "MakeResultErr",
        "ResultIsOk",
        "ResultUnwrapOk",
        "ResultUnwrapErr",
    ] {
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(operation)));
    }
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message.contains("width: W64")));
}

#[test]
fn module_verifier_checks_variant_operation_contracts() {
    let (_, _, mut fir) = pipeline(
        r#"
        module test.boundary_variant_operations;
        enum Color { Red, Green }
        fn red() -> Color { return Color::Red; }
        fn choose(value: Color) -> u32 {
            return match (value) {
                Color::Red => 1u32,
                Color::Green => 2u32,
            };
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);

    let invalid_result_type = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let mut changed = 0;
    for function in fir.module.functions.values_mut() {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if matches!(
                    instruction.kind,
                    FirInstructionKind::Variant { .. } | FirInstructionKind::VariantIs { .. }
                ) {
                    let result = instruction.result.expect("variant-operation result");
                    function
                        .value_types
                        .insert(result, invalid_result_type.clone());
                    changed += 1;
                }
            }
        }
    }
    assert!(changed >= 3);

    let diagnostics = verify_fir_module(&fir.module)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-variant-operation")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), changed, "{diagnostics:?}");
    for operation in ["Variant", "VariantIs"] {
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains(operation)));
    }
    assert!(diagnostics
        .iter()
        .all(|diagnostic| diagnostic.message.contains("width: W32")));
}

#[test]
fn definition_aware_module_verifier_checks_aggregate_construction() {
    let (mut fir, definitions) = pipeline_with_type_definitions(
        r#"
        module test.boundary_make_aggregate;
        struct Packet { kind: u8; count: u32; }
        fn packet() -> Packet { return Packet{kind: 7u8, count: 9u32}; }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    assert!(verify_fir_module_with_types(&fir.module, &definitions).is_empty());

    let instruction = fir
        .module
        .functions
        .values_mut()
        .flat_map(|function| function.blocks.iter_mut())
        .flat_map(|block| block.instructions.iter_mut())
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::MakeAggregate { .. }))
        .expect("aggregate construction");
    let FirInstructionKind::MakeAggregate { fields, .. } = &mut instruction.kind else {
        unreachable!();
    };
    fields[0].0 = "undeclared".into();

    let diagnostics = verify_fir_module_with_types(&fir.module, &definitions)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-make-aggregate")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("MakeAggregate"));
    assert!(diagnostics[0]
        .message
        .contains("field `undeclared` is not declared"));
}

#[test]
fn definition_aware_module_verifier_checks_field_extraction() {
    let (mut fir, definitions) = pipeline_with_type_definitions(
        r#"
        module test.boundary_extract_field;
        tagged Packet { Count { value: u32; }, Empty }
        fn count(packet: Packet) -> u32 {
            return match (packet) {
                Packet::Count{value} => value,
                Packet::Empty => 0u32,
            };
        }
        "#,
    );
    assert!(fir.diagnostics.is_empty(), "{:?}", fir.diagnostics);
    assert!(verify_fir_module_with_types(&fir.module, &definitions).is_empty());

    let instruction = fir
        .module
        .functions
        .values_mut()
        .flat_map(|function| function.blocks.iter_mut())
        .flat_map(|block| block.instructions.iter_mut())
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::ExtractField { .. }))
        .expect("field extraction");
    let FirInstructionKind::ExtractField { field, .. } = &mut instruction.kind else {
        unreachable!();
    };
    *field = "undeclared".into();

    let diagnostics = verify_fir_module_with_types(&fir.module, &definitions)
        .into_iter()
        .filter(|diagnostic| diagnostic.code == "fir/verify-extract-field")
        .collect::<Vec<_>>();
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(diagnostics[0].message.contains("ExtractField"));
    assert!(diagnostics[0]
        .message
        .contains("field `undeclared` is not declared"));
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
