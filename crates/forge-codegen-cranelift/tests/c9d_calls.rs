use std::collections::BTreeMap;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty, TypeDefinition,
    TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn field(name: &str, index: u32, ty: Ty) -> TypeFieldDefinition {
    TypeFieldDefinition {
        name: name.into(),
        ty,
        declaration_index: index,
    }
}

fn pair_defs() -> TypeDefinitionTable {
    let owner = DefId(100);
    BTreeMap::from([(
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::Struct {
                fields: vec![
                    field("a", 0, u(IntWidth::W32)),
                    field("b", 1, u(IntWidth::W32)),
                ],
            },
        },
    )])
}

fn scalar_const(span: Span, result: FirValueId, text: &str) -> FirInstruction {
    FirInstruction {
        span,
        result: Some(result),
        kind: FirInstructionKind::Const {
            value: FirConst::Integer { text: text.into() },
        },
    }
}

fn identity_function(owner: DefId, ty: Ty) -> FirFunction {
    let span = Span::new(0, 0);
    let local = FirLocalId(0);
    let loaded = FirValueId(0);
    FirFunction {
        owner,
        params: vec![local],
        return_type: ty.clone(),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: ty.clone(),
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(loaded),
                kind: FirInstructionKind::Load {
                    place: forge_fir::FirPlace::Local { local },
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([(loaded, ty)]),
    }
}

fn assert_prepares_both(module: &FirModule, defs: &TypeDefinitionTable) {
    for backend in [
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        CraneliftBackend::riscv64().expect("RISC-V64 backend"),
    ] {
        backend
            .prepare_module_with_types(module, defs)
            .expect("C9d aggregate call module should lower and verify");
    }
}

#[test]
fn direct_small_aggregate_round_trip_uses_flattened_piece() {
    let span = Span::new(0, 0);
    let pair = Ty::Nominal(DefId(100));
    let callee_id = DefId(1);
    let caller_id = DefId(2);
    let callee = identity_function(callee_id, pair.clone());

    let a = FirValueId(0);
    let b = FirValueId(1);
    let made = FirValueId(2);
    let returned = FirValueId(3);
    let extracted = FirValueId(4);
    let caller = FirFunction {
        owner: caller_id,
        params: vec![],
        return_type: u(IntWidth::W32),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, a, "11"),
                scalar_const(span, b, "22"),
                FirInstruction {
                    span,
                    result: Some(made),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: pair.clone(),
                        variant: None,
                        fields: vec![("a".into(), a), ("b".into(), b)],
                    },
                },
                FirInstruction {
                    span,
                    result: Some(returned),
                    kind: FirInstructionKind::Call {
                        target: callee_id,
                        args: vec![made],
                        tail: false,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(extracted),
                    kind: FirInstructionKind::ExtractField {
                        base: returned,
                        field: "b".into(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(extracted),
            }),
        }],
        value_types: BTreeMap::from([
            (a, u(IntWidth::W32)),
            (b, u(IntWidth::W32)),
            (made, pair.clone()),
            (returned, pair.clone()),
            (extracted, u(IntWidth::W32)),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_id, callee);
    module.functions.insert(caller_id, caller);
    let defs = pair_defs();
    assert_prepares_both(&module, &defs);

    let backend = CraneliftBackend::aarch64().unwrap();
    let prepared = backend.prepare_module_with_types(&module, &defs).unwrap();
    let callee_clif = prepared.function(callee_id).unwrap().display().to_string();
    assert!(callee_clif.contains("(i64) -> i64"), "{callee_clif}");
    let caller_clif = prepared.function(caller_id).unwrap().display().to_string();
    assert!(caller_clif.contains("call"), "{caller_clif}");
}

#[test]
fn five_word_aggregate_uses_hidden_return_and_indirect_parameter() {
    let span = Span::new(0, 0);
    let array_ty = Ty::Array {
        element: Box::new(u(IntWidth::W64)),
        length: Some(5),
    };
    let callee_id = DefId(10);
    let caller_id = DefId(11);
    let callee = identity_function(callee_id, array_ty.clone());

    let elements = [
        FirValueId(0),
        FirValueId(1),
        FirValueId(2),
        FirValueId(3),
        FirValueId(4),
    ];
    let made = FirValueId(5);
    let returned = FirValueId(6);
    let index = FirValueId(7);
    let extracted = FirValueId(8);
    let mut instructions = elements
        .iter()
        .enumerate()
        .map(|(index, value)| scalar_const(span, *value, &(index + 1).to_string()))
        .collect::<Vec<_>>();
    instructions.extend([
        FirInstruction {
            span,
            result: Some(made),
            kind: FirInstructionKind::MakeArray {
                items: elements.to_vec(),
            },
        },
        FirInstruction {
            span,
            result: Some(returned),
            kind: FirInstructionKind::Call {
                target: callee_id,
                args: vec![made],
                tail: false,
            },
        },
        scalar_const(span, index, "4"),
        FirInstruction {
            span,
            result: Some(extracted),
            kind: FirInstructionKind::IndexUnchecked {
                base: returned,
                index,
            },
        },
    ]);
    let mut value_types = BTreeMap::new();
    for value in elements {
        value_types.insert(value, u(IntWidth::W64));
    }
    value_types.insert(made, array_ty.clone());
    value_types.insert(returned, array_ty.clone());
    value_types.insert(index, u(IntWidth::Pointer));
    value_types.insert(extracted, u(IntWidth::W64));

    let caller = FirFunction {
        owner: caller_id,
        params: vec![],
        return_type: u(IntWidth::W64),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions,
            terminator: Some(FirTerminator::Return {
                value: Some(extracted),
            }),
        }],
        value_types,
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_id, callee);
    module.functions.insert(caller_id, caller);
    let defs = BTreeMap::new();
    assert_prepares_both(&module, &defs);

    let backend = CraneliftBackend::aarch64().unwrap();
    let prepared = backend.prepare_module_with_types(&module, &defs).unwrap();
    let callee_clif = prepared.function(callee_id).unwrap().display().to_string();
    assert!(callee_clif.contains("(i64, i64)"), "{callee_clif}");
    assert!(!callee_clif.contains("-> i64"), "{callee_clif}");
}

#[test]
fn aggregate_function_pointer_uses_same_c9_signature() {
    let span = Span::new(0, 0);
    let pair = Ty::Nominal(DefId(100));
    let callee_id = DefId(20);
    let caller_id = DefId(21);
    let callee = identity_function(callee_id, pair.clone());
    let function_ty = Ty::Function {
        params: vec![pair.clone()],
        result: Box::new(pair.clone()),
        named_arguments: false,
    };

    let function_ref = FirValueId(0);
    let a = FirValueId(1);
    let b = FirValueId(2);
    let made = FirValueId(3);
    let returned = FirValueId(4);
    let extracted = FirValueId(5);
    let caller = FirFunction {
        owner: caller_id,
        params: vec![],
        return_type: u(IntWidth::W32),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(function_ref),
                    kind: FirInstructionKind::FunctionRef { target: callee_id },
                },
                scalar_const(span, a, "7"),
                scalar_const(span, b, "9"),
                FirInstruction {
                    span,
                    result: Some(made),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: pair.clone(),
                        variant: None,
                        fields: vec![("a".into(), a), ("b".into(), b)],
                    },
                },
                FirInstruction {
                    span,
                    result: Some(returned),
                    kind: FirInstructionKind::CallIndirect {
                        callee: function_ref,
                        args: vec![made],
                        tail: false,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(extracted),
                    kind: FirInstructionKind::ExtractField {
                        base: returned,
                        field: "a".into(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(extracted),
            }),
        }],
        value_types: BTreeMap::from([
            (function_ref, function_ty),
            (a, u(IntWidth::W32)),
            (b, u(IntWidth::W32)),
            (made, pair.clone()),
            (returned, pair),
            (extracted, u(IntWidth::W32)),
        ]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_id, callee);
    module.functions.insert(caller_id, caller);
    let defs = pair_defs();
    assert_prepares_both(&module, &defs);

    let backend = CraneliftBackend::riscv64().unwrap();
    let prepared = backend.prepare_module_with_types(&module, &defs).unwrap();
    let caller_clif = prepared.function(caller_id).unwrap().display().to_string();
    assert!(caller_clif.contains("call_indirect"), "{caller_clif}");
}

#[test]
fn void_call_accepts_frontend_void_result_value() {
    let span = Span::new(0, 0);
    let callee_id = DefId(30);
    let caller_id = DefId(31);
    let call_result = FirValueId(0);

    let callee = FirFunction {
        owner: callee_id,
        params: vec![],
        return_type: Ty::Void,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::new(),
    };

    let caller = FirFunction {
        owner: caller_id,
        params: vec![],
        return_type: Ty::Void,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(call_result),
                kind: FirInstructionKind::Call {
                    target: callee_id,
                    args: vec![],
                    tail: false,
                },
            }],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(call_result, Ty::Void)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(callee_id, callee);
    module.functions.insert(caller_id, caller);
    assert_prepares_both(&module, &BTreeMap::new());
}
