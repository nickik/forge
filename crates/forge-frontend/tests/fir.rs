use forge_frontend::{
    lower_fir, lower_module, lower_resolved_bodies, parse_source, type_check_module, FirConst,
    FirInstructionKind, FirTerminator, IntWidth, OverflowMode, Ty, TypedExprKind,
};

fn lower(source: &str) -> forge_frontend::FirOutput {
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
        "body hir: {:?}",
        bodies.diagnostics
    );
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(
        typed.diagnostics.is_empty(),
        "typed: {:?}",
        typed.diagnostics
    );
    lower_fir(&bodies, &typed)
}

fn instructions(output: &forge_frontend::FirOutput) -> impl Iterator<Item = &FirInstructionKind> {
    output
        .module
        .functions
        .values()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .map(|i| &i.kind)
}

#[test]
fn value_for_array_and_slice_lower_to_explicit_iteration_cfg() {
    let output = lower(
        r#"
        module test.fir_value_for;
        type Values = u32[];
        fn sum(values: Values) -> u32 {
            var total: u32 = 0u32;
            for (val value in values) {
                if (value == 0u32) { continue; }
                total = total + value;
                if (total > 10u32) { break; }
            }
            return total;
        }
        fn main() -> i32 {
            val values: [u32; 4] = [1u32, 0u32, 4u32, 8u32];
            for (val value in values) {
                if (value == 8u32) { break; }
            }
            return i32(sum(Values(&values)));
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Len { .. }))
            .count(),
        1
    );
    assert_eq!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::IndexUnchecked { .. }))
            .count(),
        2
    );
    assert!(output.module.functions.values().all(|function| function
        .blocks
        .iter()
        .all(|block| block.terminator.is_some())));
}

#[test]
fn array_literal_fir_preserves_checked_length_and_element_type() {
    let output = lower(
        r#"
        module test.fir_array_literal;
        fn values() -> [u16; 3] { return [1u16, 2u16, 3u16]; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let instruction = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::MakeArray { .. }))
        .expect("make-array instruction");
    let FirInstructionKind::MakeArray { items } = &instruction.kind else {
        unreachable!();
    };
    assert_eq!(items.len(), 3);
    let result = instruction.result.expect("make-array result");
    assert_eq!(
        function.value_types[&result],
        Ty::Array {
            element: Box::new(Ty::Int {
                signed: false,
                width: IntWidth::W16,
            }),
            length: Some(3),
        }
    );
}

#[test]
fn mutable_global_assignment_and_address_lower_to_explicit_fir_operations() {
    let output = lower(
        r#"
        module test.fir_mutable_global;
        var COUNTER: i32 = 1i32;
        fn write(value: &mut i32) { *value = 9i32; }
        fn main() -> i32 {
            COUNTER = 3i32;
            write(&mut COUNTER);
            return COUNTER;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let counter = *output.module.globals.keys().next().expect("global");
    assert!(output.module.globals[&counter].mutable);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::StoreGlobal { global, .. } if *global == counter
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::AddressOfGlobal {
            global,
            mutable: true,
        } if *global == counter
    )));
}

#[test]
fn mutable_aggregate_global_assignment_and_address_keep_the_global_identity() {
    let output = lower(
        r#"
        module test.fir_mutable_aggregate_global;
        struct Pair { left: i32; right: i32; }
        var STATE: Pair = Pair{left: 1i32, right: 2i32};
        fn update(value: &mut Pair) { value.left = 5i32; }
        fn main() -> i32 {
            STATE = Pair{left: 3i32, right: 4i32};
            update(&mut STATE);
            return STATE.left + STATE.right;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let state = *output.module.globals.keys().next().expect("global");
    assert!(output.module.globals[&state].mutable);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::StoreGlobal { global, .. } if *global == state
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::AddressOfGlobal {
            global,
            mutable: true,
        } if *global == state
    )));
}

#[test]
fn direct_aggregate_global_field_places_lower_through_global_addresses() {
    let output = lower(
        r#"
        module test.fir_direct_aggregate_global_place;
        struct Pair { left: i32; right: i32; }
        var STATE: Pair = Pair{left: 1i32, right: 2i32};
        fn main() -> i32 {
            STATE.left = 3i32;
            val right: &mut i32 = &mut STATE.right;
            *right = 4i32;
            return STATE.left + STATE.right;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let state = *output.module.globals.keys().next().expect("global");
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::AddressOfGlobal { global, .. } if *global == state
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Store {
            place: forge_frontend::FirPlace::Field { .. },
            ..
        }
    )));
}

#[test]
fn lowers_checked_arithmetic_and_cfg() {
    let output = lower(
        r#"
        module test.fir_arithmetic;
        fn add(a: u32, b: u32) -> u32 {
            var x: u32 = a + b;
            if (x > 10u32) { x = x - 1u32; }
            while (x < 20u32) { x = x + 1u32; }
            return x;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Add,
            overflow: Some(OverflowMode::Checked),
            ..
        }
    )));
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.len() >= 6);
    assert!(function.blocks.iter().all(|b| b.terminator.is_some()));
}

#[test]
fn overflow_metadata_selects_wrapping_fir_operation() {
    let output = lower(
        r#"
        module test.fir_wrap;
        @overflow(wrap)
        fn add(a: u32, b: u32) -> u32 { return a + b; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Add,
            overflow: Some(OverflowMode::Wrapping),
            ..
        }
    )));
}

#[test]
fn indexing_has_explicit_bounds_check() {
    let output = lower(
        r#"
        module test.fir_bounds;
        fn read(values: [u32; 4], index: usize) -> u32 { return values[index]; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let usize_ty = Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    };
    let u32_ty = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let bounds = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::BoundsCheck { .. }))
        .expect("bounds-check instruction");
    let FirInstructionKind::BoundsCheck { index, len } = &bounds.kind else {
        unreachable!();
    };
    assert!(bounds.result.is_none());
    assert_eq!(function.value_types[index], usize_ty);
    assert_eq!(function.value_types[len], usize_ty);

    let indexed = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::IndexUnchecked { .. }))
        .expect("index-unchecked instruction");
    let FirInstructionKind::IndexUnchecked { base, index } = &indexed.kind else {
        unreachable!();
    };
    assert_eq!(
        function.value_types[base],
        Ty::Array {
            element: Box::new(u32_ty.clone()),
            length: Some(4),
        }
    );
    assert_eq!(function.value_types[index], usize_ty);
    assert_eq!(
        function.value_types[&indexed.result.expect("index result")],
        u32_ty
    );
}

#[test]
fn result_try_is_explicit_cfg_with_error_return() {
    let output = lower(
        r#"
        module test.fir_try;
        fn pass(value: Result[u32, u8]) -> Result[u32, u8] {
            value?;
            return value;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultIsOk { .. })));
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultUnwrapErr { .. }))
    );
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeResultErr { .. })));
    let function = output.module.functions.values().next().unwrap();
    assert!(
        function
            .blocks
            .iter()
            .filter(|b| matches!(b.terminator, Some(FirTerminator::Return { .. })))
            .count()
            >= 2
    );
}

#[test]
fn explicit_option_and_result_constructors_reach_fir() {
    let output = lower(
        r#"
        module test.fir_sum_constructors;
        fn some() -> u32? { return Some(7u32); }
        fn ok() -> Result[u32, u8] { return Ok(9u32); }
        fn err() -> Result[u32, u8] { return Err(3u8); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let integer = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    let optional_type = Ty::Optional {
        inner: Box::new(integer.clone()),
    };
    let (some_function, some_instruction) = output
        .module
        .functions
        .values()
        .find_map(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .find(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeSome { .. })
                })
                .map(|instruction| (function, instruction))
        })
        .unwrap();
    let FirInstructionKind::MakeSome {
        value: some_payload,
    } = &some_instruction.kind
    else {
        unreachable!();
    };
    assert_eq!(
        some_function
            .value_types
            .get(&some_instruction.result.unwrap()),
        Some(&optional_type)
    );
    assert_eq!(some_function.value_types.get(some_payload), Some(&integer));

    let byte = Ty::Int {
        signed: false,
        width: IntWidth::W8,
    };
    let result_type = Ty::Result {
        ok: Box::new(integer.clone()),
        error: Box::new(byte.clone()),
    };
    let (ok_function, ok_instruction) = output
        .module
        .functions
        .values()
        .find_map(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .find(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeResultOk { .. })
                })
                .map(|instruction| (function, instruction))
        })
        .unwrap();
    let FirInstructionKind::MakeResultOk { value: ok_payload } = &ok_instruction.kind else {
        unreachable!();
    };
    assert_eq!(
        ok_function.value_types.get(&ok_instruction.result.unwrap()),
        Some(&result_type)
    );
    assert_eq!(ok_function.value_types.get(ok_payload), Some(&integer));

    let (err_function, err_instruction) = output
        .module
        .functions
        .values()
        .find_map(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .find(|instruction| {
                    matches!(&instruction.kind, FirInstructionKind::MakeResultErr { .. })
                })
                .map(|instruction| (function, instruction))
        })
        .unwrap();
    let FirInstructionKind::MakeResultErr { error: err_payload } = &err_instruction.kind else {
        unreachable!();
    };
    assert_eq!(
        err_function
            .value_types
            .get(&err_instruction.result.unwrap()),
        Some(&result_type)
    );
    assert_eq!(err_function.value_types.get(err_payload), Some(&byte));
}

#[test]
fn result_patterns_lower_discriminant_tests_and_both_payload_projections() {
    let output = lower(
        r#"
        module test.fir_result_match;
        fn inspect(value: Result[u32, u8]) -> u32 {
            return match (value) {
                Ok(payload) => payload,
                Err(error) => u32(error),
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::ResultIsOk { .. }))
            .count()
            >= 2
    );
    let function = output.module.functions.values().next().unwrap();
    let test = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(&instruction.kind, FirInstructionKind::ResultIsOk { .. }))
        .unwrap();
    assert_eq!(
        function.value_types.get(&test.result.unwrap()),
        Some(&Ty::Bool)
    );
    let unwrap_ok = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(&instruction.kind, FirInstructionKind::ResultUnwrapOk { .. }))
        .unwrap();
    assert_eq!(
        function.value_types.get(&unwrap_ok.result.unwrap()),
        Some(&Ty::Int {
            signed: false,
            width: IntWidth::W32,
        })
    );
    let unwrap_err = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::ResultUnwrapErr { .. }
            )
        })
        .unwrap();
    assert_eq!(
        function.value_types.get(&unwrap_err.result.unwrap()),
        Some(&Ty::Int {
            signed: false,
            width: IntWidth::W8,
        })
    );
}

#[test]
fn payloadless_result_constructors_lower_through_unit_payloads() {
    let output = lower(
        r#"
        module test.fir_payloadless_result;
        fn ok() -> Result[void, u8] { return Ok(); }
        fn err() -> Result[u8, void] { return Err(); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Unit))
            .count(),
        2
    );
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeResultOk { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeResultErr { .. })));
}

#[test]
fn optional_promotion_is_retained_and_lowered_to_some() {
    let parsed = parse_source(
        r#"
        module test.fir_optional;
        fn maybe(value: u32) -> u32? { return value; }
        "#,
    );
    assert!(parsed.diagnostics.is_empty());
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    assert!(typed.functions.values().any(|body| body
        .expressions
        .iter()
        .any(|expr| matches!(expr.kind, TypedExprKind::OptionalPromote { .. }))));
    let output = lower_fir(&bodies, &typed);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeSome { .. })));
}

#[test]
fn method_reference_receiver_becomes_explicit_address() {
    let output = lower(
        r#"
        module test.fir_method;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
        }
        fn main() -> i32 {
            val point = Point{x: 7i32};
            return point.get();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::AddressOf { mutable: false, .. })));
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::Call { args, .. } if !args.is_empty())));
}

#[test]
fn local_address_preserves_reference_type_and_mutability() {
    let output = lower(
        r#"
        module test.fir_address;
        fn read(value: u32) -> u32 {
            val address: &u32 = &value;
            return *address;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let (function, instruction) = output
        .module
        .functions
        .values()
        .flat_map(|function| {
            function.blocks.iter().flat_map(move |block| {
                block
                    .instructions
                    .iter()
                    .map(move |instruction| (function, instruction))
            })
        })
        .find(|(_, instruction)| matches!(&instruction.kind, FirInstructionKind::AddressOf { .. }))
        .expect("address-of instruction");
    let FirInstructionKind::AddressOf {
        place: forge_frontend::FirPlace::Local { local },
        mutable,
    } = &instruction.kind
    else {
        unreachable!();
    };
    let result = instruction.result.expect("address-of result");
    assert!(!*mutable);
    assert_eq!(
        function.locals.get(local).map(|local| &local.ty),
        Some(&Ty::Int {
            signed: false,
            width: IntWidth::W32,
        })
    );
    assert_eq!(
        function.value_types.get(&result),
        Some(&Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Int {
                signed: false,
                width: IntWidth::W32,
            }),
        })
    );
}

#[test]
fn local_stores_preserve_the_declared_local_type() {
    let output = lower(
        r#"
        module test.fir_local_store;
        fn update(value: u32) -> u32 {
            var copy: u32 = value;
            copy = 7u32;
            return copy;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let mut stores = 0;
    for function in output.module.functions.values() {
        for instruction in function
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
        {
            let FirInstructionKind::Store {
                place: forge_frontend::FirPlace::Local { local },
                value,
            } = &instruction.kind
            else {
                continue;
            };
            stores += 1;
            assert_eq!(function.locals[local].ty, function.value_types[value]);
        }
    }
    assert!(stores > 0);
}

#[test]
fn defer_call_is_emitted_before_return() {
    let output = lower(
        r#"
        module test.fir_defer;
        fn cleanup() -> void { return; }
        fn main() -> i32 {
            defer cleanup();
            return 7i32;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output
        .module
        .functions
        .values()
        .find(|f| {
            f.return_type
                == Ty::Int {
                    signed: true,
                    width: forge_frontend::IntWidth::W32,
                }
        })
        .unwrap();
    let return_block = main
        .blocks
        .iter()
        .find(|b| matches!(b.terminator, Some(FirTerminator::Return { .. })))
        .unwrap();
    assert!(return_block
        .instructions
        .iter()
        .any(|i| matches!(i.kind, FirInstructionKind::Call { .. })));
}

#[test]
fn defer_cleanup_control_flow_is_rejected_in_fir() {
    let output = lower(
        r#"
        module test.fir_defer_cleanup_control;
        fn main() -> i32 {
            defer { return 7i32; }
            return 0i32;
        }
        "#,
    );
    assert!(
        output
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "fir/control-in-cleanup"),
        "expected defer cleanup control-flow diagnostic: {:?}",
        output.diagnostics
    );
}

#[test]
fn distinct_conversions_use_dedicated_fir_operations() {
    let output = lower(
        r#"
        module test.fir_distinct_conversion;
        distinct UserId: u32;
        fn main() -> u32 {
            val user: UserId = UserId(7u32);
            return u32(user);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::DistinctFromUnderlying { .. }
    )));
    assert!(instructions(&output)
        .any(|instruction| matches!(instruction, FirInstructionKind::DistinctToUnderlying { .. })));
}

#[test]
fn defer_cleanups_are_emitted_for_loop_transfers_and_try_return() {
    let output = lower(
        r#"
        module test.fir_defer_control_flow;
        fn cleanup() -> void { return; }
        fn fallible(ok: bool) -> Result[void, void] {
            if (ok) { return Ok(); }
            return Err();
        }
        fn main() -> Result[void, void] {
            var index: i32 = 0i32;
            while (index < 3i32) {
                defer cleanup();
                index = index + 1i32;
                if (index == 1i32) { continue; }
                if (index == 2i32) { break; }
            }
            defer cleanup();
            fallible(false)?;
            return Ok();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output
        .module
        .functions
        .values()
        .filter(|function| matches!(&function.return_type, Ty::Result { .. }))
        .max_by_key(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| block.instructions.iter())
                .filter(|instruction| matches!(instruction.kind, FirInstructionKind::Call { .. }))
                .count()
        })
        .expect("Result-returning main function");
    let cleanup_calls = main
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| matches!(instruction.kind, FirInstructionKind::Call { .. }))
        .count();
    assert!(
        cleanup_calls >= 3,
        "expected cleanup calls for continue, break, and ? return; got {cleanup_calls}"
    );
    assert!(main.blocks.iter().any(|block| {
        matches!(block.terminator, Some(FirTerminator::Return { .. }))
            && block
                .instructions
                .iter()
                .any(|instruction| matches!(instruction.kind, FirInstructionKind::Call { .. }))
    }));
}

#[test]
fn boolean_match_plan_lowers_to_cfg() {
    let output = lower(
        r#"
        module test.fir_bool_match;
        fn choose(value: bool) -> i32 {
            return match (value) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.len() >= 5);
    assert!(function
        .blocks
        .iter()
        .all(|block| block.terminator.is_some()));
    assert!(function
        .blocks
        .iter()
        .any(|block| matches!(block.terminator, Some(FirTerminator::Branch { .. }))));
}

#[test]
fn boolean_match_preserves_wildcard_and_guard_fallthrough() {
    let output = lower(
        r#"
        module test.fir_bool_guard;
        fn choose(value: bool, guard: bool) -> i32 {
            return match (value) {
                true when guard => 1i32,
                true => 2i32,
                _ => 3i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let branches = output
        .module
        .functions
        .values()
        .flat_map(|function| &function.blocks)
        .filter(|block| matches!(block.terminator, Some(FirTerminator::Branch { .. })))
        .count();
    assert!(branches >= 3, "expected pattern and guard branches");
}

#[test]
fn boolean_match_scrutinee_is_evaluated_once() {
    let output = lower(
        r#"
        module test.fir_bool_once;
        fn source() -> bool { return true; }
        fn choose() -> i32 {
            return match (source()) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let calls = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::Call { .. }))
        .count();
    assert_eq!(calls, 1, "match scrutinee call must execute once");
}

#[test]
fn boolean_match_block_arms_lower_as_void_cfg() {
    let output = lower(
        r#"
        module test.fir_bool_blocks;
        fn choose(value: bool) -> void {
            match (value) {
                true => { val x: u32 = 1u32; },
                false => { val y: u32 = 2u32; },
            };
            return;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Unit)));
}

#[test]
fn optional_match_extracts_payload_before_guard_and_body() {
    let output = lower(
        r#"
        module test.fir_option_match;
        fn choose(value: u32?) -> u32 {
            return match (value) {
                Some(x) when x > 10u32 => x,
                Some(x) => x,
                None => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionIsSome { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionUnwrap { .. })));
    let function = output.module.functions.values().next().unwrap();
    let unwrap = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(&instruction.kind, FirInstructionKind::OptionUnwrap { .. }))
        .unwrap();
    assert_eq!(
        function.value_types.get(&unwrap.result.unwrap()),
        Some(&Ty::Int {
            signed: false,
            width: IntWidth::W32,
        })
    );
}

#[test]
fn enum_match_uses_resolved_variant_tests() {
    let output = lower(
        r#"
        module test.fir_enum_match;
        enum Color { Red, Green, Blue }
        fn choose(value: Color) -> i32 {
            return match (value) {
                Color::Red => 1i32,
                Color::Green => 2i32,
                Color::Blue => 3i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let tests = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::VariantIs { .. }))
        .count();
    assert_eq!(tests, 3);
    let function = output.module.functions.values().next().unwrap();
    for instruction in function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .filter(|instruction| matches!(&instruction.kind, FirInstructionKind::VariantIs { .. }))
    {
        assert_eq!(
            function.value_types.get(&instruction.result.unwrap()),
            Some(&Ty::Bool)
        );
    }
}

#[test]
fn fieldless_and_payload_variants_use_distinct_fir_constructors() {
    let output = lower(
        r#"
        module test.fir_variant_constructors;
        tagged Token {
            Number { value: u32; },
            Empty,
        }
        fn empty() -> Token { return Token::Empty; }
        fn number() -> Token { return Token::Number{value: 7u32}; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Variant { name, .. } if name == "Empty"
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::MakeAggregate {
            variant: Some(name),
            ..
        } if name == "Number"
    )));
    assert!(!instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Variant { name, .. } if name == "Number"
    )));
}

#[test]
fn aggregate_constructor_fir_preserves_checked_field_payload_types() {
    let output = lower(
        r#"
        module test.fir_aggregate_field_types;
        struct Packet { kind: u8; count: u32; }
        fn packet() -> Packet { return Packet{kind: 7u8, count: 9u32}; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let fields = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find_map(|instruction| match &instruction.kind {
            FirInstructionKind::MakeAggregate { fields, .. } => Some(fields),
            _ => None,
        })
        .expect("aggregate constructor");
    assert_eq!(
        fields
            .iter()
            .map(|(name, value)| (name.as_str(), function.value_types.get(value)))
            .collect::<Vec<_>>(),
        vec![
            (
                "kind",
                Some(&Ty::Int {
                    signed: false,
                    width: IntWidth::W8,
                }),
            ),
            (
                "count",
                Some(&Ty::Int {
                    signed: false,
                    width: IntWidth::W32,
                }),
            ),
        ]
    );
}

#[test]
fn aggregate_constructor_materializes_defaulted_fields_before_fir() {
    let output = lower(
        r#"
        module test.fir_aggregate_defaults;
        struct Packet { kind: u8; count: u32 = 9u32; }
        tagged Message { Pair { left: u16; right: u16 = 7u16; }, }
        fn packet() -> Packet { return Packet{kind: 3u8}; }
        fn message() -> Message { return Message::Pair{left: 5u16}; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let constructors = output
        .module
        .functions
        .values()
        .flat_map(|function| {
            function
                .blocks
                .iter()
                .flat_map(|block| &block.instructions)
                .filter_map(|instruction| match &instruction.kind {
                    FirInstructionKind::MakeAggregate {
                        variant, fields, ..
                    } => Some((variant.as_deref(), fields)),
                    _ => None,
                })
        })
        .collect::<Vec<_>>();
    assert!(constructors.iter().any(|(variant, fields)| {
        variant.is_none()
            && fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                == vec!["kind", "count"]
    }));
    assert!(constructors.iter().any(|(variant, fields)| {
        *variant == Some("Pair")
            && fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>()
                == vec!["left", "right"]
    }));
}

#[test]
fn tagged_match_extracts_typed_payload_bindings() {
    let output = lower(
        r#"
        module test.fir_tagged_match;
        tagged Token {
            Number { value: u32; },
            Empty,
        }
        fn read(token: Token, gate: bool) -> u32 {
            return match (token) {
                Token::Number{value} when gate => value,
                Token::Number{value} => value,
                Token::Empty => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::VariantIs { .. })));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "value"
    )));
}

#[test]
fn scalar_integer_literal_match_lowers_to_equality() {
    let output = lower(
        r#"
        module test.fir_scalar_integer;
        fn choose(value: i32) -> i32 {
            return match (value) { 7 => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Eq,
            ..
        }
    )));
}

#[test]
fn scalar_char_range_match_lowers_both_bounds() {
    let output = lower(
        r#"
        module test.fir_scalar_range;
        fn choose(value: char) -> i32 {
            return match (value) { 'a'..='z' => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::GreaterEq,
            ..
        }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::LessEq,
            ..
        }
    )));
}

#[test]
fn scalar_string_literal_match_is_resolved_before_fir() {
    let output = lower(
        r#"
        module test.fir_scalar_string;
        fn choose(value: str) -> i32 {
            return match (value) { "yes" => 1i32, _ => 0i32, };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/pattern-decision-tree-missing"));
}

#[test]
fn struct_pattern_lowers_nested_field_test_and_binding() {
    let output = lower(
        r#"
        module test.fir_struct_pattern;
        struct Point { x: i32; y: i32; }
        fn choose(point: Point) -> i32 {
            return match (point) {
                Point{x: 7, y} => y,
                _ => 0i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "x"
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::ExtractField { field, .. } if field == "y"
    )));
}

#[test]
fn sequence_rest_pattern_lowers_length_index_and_tail() {
    let output = lower(
        r#"
        module test.fir_sequence_pattern;
        fn choose(values: u32[]) -> u32 {
            return match (values) {
                [first, second, ..rest] => first + second,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let len = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| matches!(instruction.kind, FirInstructionKind::Len { .. }))
        .expect("len instruction");
    assert_eq!(
        function.value_types[&len.result.expect("len result")],
        Ty::Int {
            signed: false,
            width: IntWidth::Pointer,
        }
    );
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::IndexUnchecked { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Subsequence { .. })));
}

#[test]
fn explicit_array_reference_slice_view_has_dedicated_fir() {
    let output = lower(
        r#"
        module test.fir_slice_view;
        type ReadSlice = u32[];
        fn read(values: ReadSlice) -> u32 { return values[0]; }
        fn main() -> i32 {
            var values: [u32; 2] = [4u32, 9u32];
            val view: ReadSlice = ReadSlice(&values);
            return i32(read(view));
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::SliceFromArrayRef { .. }))
    );
}

#[test]
fn or_pattern_uses_separate_resolved_alternatives() {
    let output = lower(
        r#"
        module test.fir_or_pattern;
        enum Token { Plus, Minus, Number }
        fn choose(token: Token) -> i32 {
            return match (token) {
                Token::Plus | Token::Minus => 1i32,
                _ => 0i32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let variants = instructions(&output)
        .filter(|op| matches!(op, FirInstructionKind::VariantIs { .. }))
        .count();
    assert_eq!(variants, 2);
}

#[test]
fn as_pattern_binds_whole_value_before_body() {
    let output = lower(
        r#"
        module test.fir_as_pattern;
        struct Point { x: i32; y: i32; }
        fn choose(point: Point) -> i32 {
            return match (point) {
                whole @ Point{x, y} => whole.x + x + y,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Store { .. }))
            .count()
            >= 3
    );
}

#[test]
fn normalized_defaults_lower_without_fir_default_reconstruction() {
    let output = lower(
        r#"
        module test.fir_defaults;
        fn source() -> u32 { return 3u32; }
        nfn combine(first: u32, second: u32 = first + 1u32, third: u32 = second + 1u32) -> u32 {
            return first + second + third;
        }
        fn main() -> u32 {
            return combine(:third = 9u32, :first = source());
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!instructions(&output).any(|op| matches!(op, FirInstructionKind::Poison)));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Call { args, .. } if args.len() == 3
    )));
    assert_eq!(
        instructions(&output)
            .filter(|op| matches!(op, FirInstructionKind::Call { args, .. } if args.is_empty()))
            .count(),
        1,
        "explicit source() argument must be evaluated exactly once"
    );
}

#[test]
fn chained_defaults_can_read_earlier_materialized_parameters() {
    let output = lower(
        r#"
        module test.fir_chained_defaults;
        nfn advance(first: u32, second: u32 = first + 1u32, third: u32 = second + 1u32) -> u32 {
            return third;
        }
        fn main() -> u32 { return advance(:first = 5u32); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Call { args, .. } if args.len() == 3
    )));
    assert!(
        instructions(&output)
            .filter(|op| matches!(
                op,
                FirInstructionKind::Binary {
                    op: forge_frontend::ast::BinaryOp::Add,
                    ..
                }
            ))
            .count()
            >= 2
    );
}

#[test]
fn captured_closure_lowers_environment_body_and_call() {
    let output = lower(
        r#"
        module test.fir_closure_capture;
        fn main() -> u32 {
            val factor: u32 = 4u32;
            val scale = [factor](x: u32) -> u32 { return x * factor; };
            return scale(3u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    assert_eq!(main.closures.len(), 1);
    let closure = main.closures.values().next().unwrap();
    assert_eq!(closure.captures.len(), 1);
    assert_eq!(closure.captures[0].mode, forge_frontend::CaptureMode::Value);
    assert!(main
        .blocks
        .iter()
        .any(|block| block.closure == Some(closure.id)));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::MakeClosure { captures, .. } if captures.len() == 1
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::CallClosure { .. })));
}

#[test]
fn mutable_reference_closure_uses_aliasing_capture_place() {
    let output = lower(
        r#"
        module test.fir_closure_mut_ref;
        fn main() -> u32 {
            var count: u32 = 0u32;
            val next = [&mut count]() -> u32 {
                count = count + 1u32;
                return count;
            };
            next();
            return count;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    let closure = main.closures.values().next().unwrap();
    assert_eq!(
        closure.captures[0].mode,
        forge_frontend::CaptureMode::MutableReference
    );
    assert!(main
        .blocks
        .iter()
        .filter(|block| block.closure == Some(closure.id))
        .any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.kind,
                    FirInstructionKind::Store {
                        place: forge_frontend::FirPlace::ClosureCapture { .. },
                        ..
                    }
                )
            })
        }));
}

#[test]
fn capture_free_closure_function_pointer_has_no_environment() {
    let output = lower(
        r#"
        module test.fir_closure_fn_ptr;
        fn main() -> u32 {
            val op: fn(u32) -> u32 = (x: u32) -> u32 { return x + 1u32; };
            return op(8u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().next().unwrap();
    let closure = main.closures.values().next().unwrap();
    assert!(closure.function_pointer);
    assert!(closure.captures.is_empty());
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::MakeClosure { captures, .. } if captures.is_empty()
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::CallIndirect { .. })));
}

#[test]
fn required_tail_calls_remain_explicit_in_direct_and_indirect_fir() {
    let output = lower(
        r#"
        module test.fir_tail_calls;
        fn add_one(value: u64) -> u64 { return value + 1u64; }
        fn direct(value: u64) -> u64 { return tail add_one(value); }
        fn indirect(op: fn(u64) -> u64, value: u64) -> u64 {
            return tail op(value);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::Call { tail: true, .. }))
    );
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::CallIndirect { tail: true, .. })));
}

#[test]
fn required_local_closure_tail_call_remains_explicit_in_fir() {
    let output = lower(
        r#"
        module test.fir_closure_tail_call;
        fn main() -> u32 {
            val offset: u32 = 7u32;
            val add = [offset](value: u32) -> u32 { return value + offset; };
            return tail add(5u32);
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::CallClosure { tail: true, .. })));
}

#[test]
fn execution_context_lowers_save_set_load_and_restore() {
    let output = lower(
        r#"
        module test.context_fir;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            with context(:scratch = &scratch) {
                val current = context.scratch;
            }
            return scratch;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let ops = instructions(&output).collect::<Vec<_>>();
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextSave {
            slot: forge_frontend::ContextSlot::Scratch
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextSet {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextLoad {
            slot: forge_frontend::ContextSlot::Scratch
        }
    )));
    assert!(ops.iter().any(|op| matches!(
        op,
        FirInstructionKind::ContextRestore {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
    assert!(!output
        .diagnostics
        .iter()
        .any(|d| d.code == "fir/context-not-resolved"));
}

#[test]
fn execution_context_restore_runs_on_return_cleanup_edge() {
    let output = lower(
        r#"
        module test.context_return;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            with context(:scratch = &scratch) {
                return scratch;
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().unwrap();
    let return_block = function
        .blocks
        .iter()
        .find(|block| matches!(block.terminator, Some(FirTerminator::Return { .. })))
        .expect("return block");
    assert!(return_block.instructions.iter().any(|instruction| matches!(
        instruction.kind,
        FirInstructionKind::ContextRestore {
            slot: forge_frontend::ContextSlot::Scratch,
            ..
        }
    )));
}

#[test]
fn nested_execution_context_restores_in_lifo_order() {
    let output = lower(
        r#"
        module test.context_nested_fir;
        fn main() -> u32 {
            var scratch: u32 = 7u32;
            var logger: u64 = 9u64;
            with context(:scratch = &scratch, :logger = &logger) {
                val a = context.scratch;
                val b = context.logger;
            }
            return scratch;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let restores = instructions(&output)
        .filter_map(|op| match op {
            FirInstructionKind::ContextRestore { slot, .. } => Some(*slot),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        restores,
        vec![
            forge_frontend::ContextSlot::Logger,
            forge_frontend::ContextSlot::Scratch,
        ]
    );
}

#[test]
fn select_lowers_to_runtime_select_cfg_with_payload_binding() {
    let output = lower(
        r#"
        module test.fir_select;
        struct Job { value: u32; }
        struct Jobs { marker: u8; }
        impl Jobs {
            fn recv(self: &Jobs) -> Job { return Job{value: 1u32}; }
        }
        fn worker(jobs: &Jobs) -> void {
            select {
                recv jobs -> Job{value} => { val copy: u32 = value; }
                timeout #duration "100ms" => { }
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let worker = output
        .module
        .functions
        .values()
        .find(|f| {
            f.blocks
                .iter()
                .any(|b| matches!(b.terminator, Some(FirTerminator::Select { .. })))
        })
        .expect("worker select FIR");
    let select = worker
        .blocks
        .iter()
        .find_map(|block| match &block.terminator {
            Some(FirTerminator::Select { operation, cases }) => Some((*operation, cases)),
            _ => None,
        })
        .unwrap();
    assert_eq!(select.0, forge_frontend::RuntimeOperationId::SelectWait);
    assert_eq!(select.1.len(), 2);
    assert!(matches!(
        select.1[0],
        forge_frontend::FirSelectCase::Receive {
            operation: forge_frontend::RuntimeOperationId::ChannelReceive,
            ..
        }
    ));
    assert!(matches!(
        select.1[1],
        forge_frontend::FirSelectCase::Timeout {
            operation: forge_frontend::RuntimeOperationId::SelectTimeout,
            ..
        }
    ));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Const { value: forge_frontend::FirConst::Duration { value } }
            if value == "100ms"
    )));
}

#[test]
fn selected_receive_struct_pattern_binds_exact_payload_fields() {
    let output = lower(
        r#"
        module test.fir_select_pattern;
        struct Job { value: u32; }
        struct Jobs { marker: u8; }
        impl Jobs {
            fn recv(self: &Jobs) -> Job { return Job{value: 1u32}; }
        }
        fn worker(jobs: &Jobs) -> void {
            select {
                recv jobs -> Job{value} => { val copy: u32 = value; }
            }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(
        |op| matches!(op, FirInstructionKind::ExtractField { field, .. } if field == "value")
    ));
}

#[test]
fn raw_pointer_dereference_lowers_with_unsafe_provenance() {
    let output = lower(
        r#"
        module test.fir_raw_deref;
        fn read(p: *u32) -> u32 {
            unsafe { return *p; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let instruction = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Load {
                    place: forge_frontend::FirPlace::RawDeref { .. }
                }
            )
        })
        .expect("raw load");
    let FirInstructionKind::Load {
        place: forge_frontend::FirPlace::RawDeref {
            address, volatile, ..
        },
    } = &instruction.kind
    else {
        unreachable!();
    };
    let pointee = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    assert!(!volatile);
    assert_eq!(
        function.value_types[address],
        Ty::Pointer {
            volatile: false,
            inner: Box::new(pointee.clone()),
        }
    );
    assert_eq!(
        function.value_types[&instruction.result.expect("raw load result")],
        pointee
    );
}

#[test]
fn raw_pointer_store_uses_provenanced_raw_place() {
    let output = lower(
        r#"
        module test.fir_raw_store;
        fn write(p: *u32, value: u32) {
            unsafe { *p = value; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let instruction = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Store {
                    place: forge_frontend::FirPlace::RawDeref { .. },
                    ..
                }
            )
        })
        .expect("raw store");
    let FirInstructionKind::Store {
        place: forge_frontend::FirPlace::RawDeref {
            address, volatile, ..
        },
        value,
    } = &instruction.kind
    else {
        unreachable!();
    };
    let pointee = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    assert!(!volatile);
    assert_eq!(
        function.value_types[address],
        Ty::Pointer {
            volatile: false,
            inner: Box::new(pointee.clone()),
        }
    );
    assert_eq!(function.value_types[value], pointee);
}

#[test]
fn pointer_offset_is_a_dedicated_provenanced_fir_operation() {
    let output = lower(
        r#"
        module test.fir_pointer_offset;
        fn previous(p: *u32) -> *u32 {
            unsafe { return p - 2usize; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let mut offsets = 0;
    for function in output.module.functions.values() {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let FirInstructionKind::PointerOffset {
                pointer,
                offset,
                subtract,
                ..
            } = &instruction.kind
            else {
                continue;
            };
            assert!(*subtract);
            let result = instruction.result.expect("pointer-offset result");
            let pointer_ty = &function.value_types[pointer];
            assert!(matches!(pointer_ty, Ty::Pointer { .. }));
            assert_eq!(&function.value_types[&result], pointer_ty);
            assert_eq!(
                function.value_types[offset],
                Ty::Int {
                    signed: false,
                    width: IntWidth::Pointer,
                }
            );
            offsets += 1;
        }
    }
    assert_eq!(offsets, 1);
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Sub,
            ..
        }
    )));
}

#[test]
fn pointer_conversions_are_not_plain_fir_converts() {
    let output = lower(
        r#"
        module test.fir_pointer_convert;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { unsafe { return usize(p); } }
        fn cast(p: *u32) -> BytePtr { unsafe { return BytePtr(p); } }
        fn from_address(address: usize) -> BytePtr { unsafe { return BytePtr(address); } }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let mut operations = Vec::new();
    for function in output.module.functions.values() {
        for instruction in function.blocks.iter().flat_map(|block| &block.instructions) {
            let FirInstructionKind::PointerConvert {
                value,
                target,
                operation,
                ..
            } = &instruction.kind
            else {
                continue;
            };
            let result = instruction.result.expect("pointer-convert result");
            assert_eq!(&function.value_types[&result], target);
            let source = &function.value_types[value];
            match operation {
                forge_frontend::UnsafeOperationKind::PointerToInteger => {
                    assert!(matches!(source, Ty::Pointer { .. }));
                    assert!(matches!(target, Ty::Int { .. } | Ty::Byte));
                }
                forge_frontend::UnsafeOperationKind::IntegerToPointer => {
                    assert!(matches!(source, Ty::Int { .. } | Ty::Byte));
                    assert!(matches!(target, Ty::Pointer { .. }));
                }
                forge_frontend::UnsafeOperationKind::PointerReinterpret => {
                    assert!(matches!(source, Ty::Pointer { .. }));
                    assert!(matches!(target, Ty::Pointer { .. }));
                    assert_ne!(source, target);
                }
                operation => panic!("unexpected pointer conversion operation {operation:?}"),
            }
            operations.push(*operation);
        }
    }
    assert_eq!(operations.len(), 3);
    assert!(operations.contains(&forge_frontend::UnsafeOperationKind::PointerToInteger));
    assert!(operations.contains(&forge_frontend::UnsafeOperationKind::IntegerToPointer));
    assert!(operations.contains(&forge_frontend::UnsafeOperationKind::PointerReinterpret));
}

#[test]
fn lossless_integer_conversion_has_dedicated_fir_semantics() {
    let output = lower(
        r#"
        module test.fir_lossless_integer_convert;
        fn widen(value: u8) -> u64 { return u64(value); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::LosslessIntegerConvert {
            target: forge_frontend::Ty::Int {
                signed: false,
                width: forge_frontend::IntWidth::W64,
            },
            ..
        }
    )));
}

#[test]
fn integer_to_float_is_a_dedicated_fir_operation() {
    let output = lower(
        r#"
        module test.fir_integer_to_float;
        fn convert(value: i32) -> f64 { return f64(value); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::IntegerToFloat {
            target: forge_frontend::Ty::Float { bits: 64 },
            ..
        }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::LosslessIntegerConvert {
            target: forge_frontend::Ty::Float { .. },
            ..
        }
    )));
}

#[test]
fn float_width_conversion_is_a_dedicated_fir_operation() {
    let output = lower(
        r#"
        module test.fir_float_convert;
        fn widen(value: f32) -> f64 { return f64(value); }
        fn narrow(value: f64) -> f32 { return f32(value); }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::FloatConvert {
            target: forge_frontend::Ty::Float { bits: 64 },
            ..
        }
    )));
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::FloatConvert {
            target: forge_frontend::Ty::Float { bits: 32 },
            ..
        }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::LosslessIntegerConvert {
            target: forge_frontend::Ty::Float { .. },
            ..
        }
    )));
}

#[test]
fn ordinary_reference_deref_remains_safe_fir_deref() {
    let output = lower(
        r#"
        module test.fir_reference_deref;
        fn read(p: &u32) -> u32 { return *p; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let function = output.module.functions.values().next().expect("function");
    let instruction = function
        .blocks
        .iter()
        .flat_map(|block| &block.instructions)
        .find(|instruction| {
            matches!(
                &instruction.kind,
                FirInstructionKind::Load {
                    place: forge_frontend::FirPlace::Deref { .. }
                }
            )
        })
        .expect("safe load");
    let FirInstructionKind::Load {
        place: forge_frontend::FirPlace::Deref { address },
    } = &instruction.kind
    else {
        unreachable!();
    };
    let pointee = Ty::Int {
        signed: false,
        width: IntWidth::W32,
    };
    assert_eq!(
        function.value_types[address],
        Ty::Reference {
            mutable: false,
            inner: Box::new(pointee.clone()),
        }
    );
    assert_eq!(
        function.value_types[&instruction.result.expect("safe load result")],
        pointee
    );
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::RawDeref { .. }
        }
    )));
}

#[test]
fn bitstruct_read_lowers_to_storage_shift_and_mask() {
    let output = lower(
        r#"
        module test.fir_bitstruct_read;
        bitstruct Status: u16 { ready: 1; error: 1; mode: 3; code: 5; reserved: 6; }
        fn mode(status: Status) -> u8 { return status.mode; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::BitStructStorage { .. }))
    );
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::ShiftRight,
            ..
        }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::BitAnd,
            ..
        }
    )));
    assert!(
        instructions(&output).any(|op| matches!(op, FirInstructionKind::BitFieldExtract { .. }))
    );
}

#[test]
fn bitstruct_write_is_checked_and_uses_read_modify_write_masks() {
    let output = lower(
        r#"
        module test.fir_bitstruct_write;
        bitstruct Status: u16 { ready: 1; error: 1; mode: 3; code: 5; reserved: 6; }
        fn update() -> u8 {
            var status: Status = Status(0u16);
            status.mode = 7u8;
            return status.mode;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::BitFieldCheck { width: 3, .. })));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::ShiftLeft,
            ..
        }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::BitOr,
            ..
        }
    )));
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::BitStructFromStorage { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::BitFieldExtend { .. })));
}

#[test]
fn one_bit_bitstruct_field_is_bool_and_needs_no_range_check() {
    let output = lower(
        r#"
        module test.fir_bitstruct_bool;
        bitstruct Flags: u8 { ready: 1; reserved: 7; }
        fn update() -> bool {
            var flags: Flags = Flags(0u8);
            flags.ready = true;
            return flags.ready;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::BitFieldCheck { width: 1, .. })));
}

#[test]
fn map_pattern_lowers_resolved_collection_protocol_operations() {
    let output = lower(
        r#"
        module test.fir_map_protocol;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? {
                if (key.len == 4usize) { return Some(1u32); }
                return None;
            }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool {
                return keys.len == 1usize;
            }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Const { value: FirConst::String { value } } if value == "name"
    )));
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::CollectionPatternLookup { .. })));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::CollectionPatternHasOnly { keys, .. } if keys.len() == 1
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Len { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionIsSome { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionUnwrap { .. })));
}

#[test]
fn map_pattern_rest_skips_closed_key_check_and_optional_binds_option() {
    let output = lower(
        r#"
        module test.fir_map_rest;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> void {
            match (map) {
                {:name name, :age age?, ..} => {},
                _ => {},
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Const { value: FirConst::String { value } } if value == "age"
    )));
    assert!(instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::CollectionPatternLookup { .. })));
    assert!(!instructions(&output)
        .any(|op| matches!(op, FirInstructionKind::CollectionPatternHasOnly { .. })));
}

#[test]
fn runtime_globals_lower_to_explicit_initializer_functions() {
    let output = lower(
        r#"
        module test.fir_global_init;
        fn seed() -> u32 { return 4u32; }
        val dependent: u32 = base + 1u32;
        val base: u32 = seed();
        const fixed: u32 = 9u32;
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert_eq!(output.module.global_initializers.len(), 2);
    assert_eq!(output.module.global_init_order.len(), 2);
    assert_eq!(
        output
            .module
            .globals
            .values()
            .filter(|global| global.constant.is_some())
            .count(),
        1
    );

    let base = output.module.global_init_order[0];
    let dependent = output.module.global_init_order[1];
    assert!(output.module.global_initializers[&base]
        .dependencies
        .is_empty());
    assert_eq!(
        output.module.global_initializers[&dependent].dependencies,
        vec![base]
    );

    let base_calls = output.module.global_initializers[&base]
        .function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter(|instruction| matches!(instruction.kind, FirInstructionKind::Call { .. }))
        .count();
    assert_eq!(
        base_calls, 1,
        "runtime initializer expression must execute once"
    );
    assert!(output.module.global_initializers[&dependent]
        .function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .any(|instruction| matches!(
            instruction.kind,
            FirInstructionKind::LoadGlobal { global } if global == base
        )));
}
