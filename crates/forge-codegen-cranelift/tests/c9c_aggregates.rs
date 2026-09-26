use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinition, TypeDefinitionKind, TypeDefinitionTable, TypeFieldDefinition,
    TypeVariantDefinition,
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

fn record_def() -> (DefId, TypeDefinition) {
    let owner = DefId(100);
    (
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::Struct {
                fields: vec![
                    field("a", 0, u(IntWidth::W8)),
                    field("b", 1, u(IntWidth::W64)),
                    field("c", 2, u(IntWidth::W16)),
                    field("d", 3, u(IntWidth::W32)),
                ],
            },
        },
    )
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

fn module_with(function: FirFunction) -> FirModule {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    module
}

fn assert_prepares(module: &FirModule, defs: &TypeDefinitionTable) {
    for backend in [
        CraneliftBackend::aarch64().expect("AArch64 backend"),
        CraneliftBackend::riscv64().expect("RISC-V64 backend"),
    ] {
        let prepared = backend
            .prepare_module_with_types(module, defs)
            .expect("C9c aggregate FIR should lower and verify");
        let function = prepared
            .function(*module.functions.keys().next().unwrap())
            .expect("lowered function");
        let clif = function.display().to_string();
        assert!(clif.contains("ss0") || clif.contains("stack"), "{clif}");
    }
}

fn assert_invalid_on_host_targets(
    function: FirFunction,
    defs: &TypeDefinitionTable,
    message: &str,
) {
    let module = module_with(function);
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, defs) {
            Ok(_) => panic!("malformed variant FIR unexpectedly lowered"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFirShape {
                message: message.into(),
            }
        );
    }
}

fn tagged_def() -> (DefId, TypeDefinition) {
    let owner = DefId(101);
    (
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::Tagged {
                variants: vec![
                    TypeVariantDefinition {
                        name: "Number".into(),
                        declaration_index: 0,
                        fields: vec![field("value", 0, u(IntWidth::W32))],
                    },
                    TypeVariantDefinition {
                        name: "Empty".into(),
                        declaration_index: 1,
                        fields: Vec::new(),
                    },
                ],
            },
        },
    )
}

fn aggregate_constructor(fields: Vec<(String, FirValueId)>, payload_ty: Ty) -> FirFunction {
    let span = Span::new(0, 0);
    let payload = FirValueId(0);
    let aggregate = FirValueId(1);
    let record_ty = Ty::Nominal(DefId(100));
    FirFunction {
        owner: DefId(5),
        params: Vec::new(),
        return_type: record_ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, payload, "1"),
                FirInstruction {
                    span,
                    result: Some(aggregate),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: record_ty.clone(),
                        variant: None,
                        fields,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(aggregate),
            }),
        }],
        value_types: BTreeMap::from([(payload, payload_ty), (aggregate, record_ty)]),
    }
}

#[test]
fn aggregate_constructor_rejects_duplicate_supplied_fields() {
    let value = FirValueId(0);
    let function = aggregate_constructor(
        vec![("a".into(), value), ("a".into(), value)],
        u(IntWidth::W8),
    );
    let defs = BTreeMap::from([record_def()]);
    assert_invalid_on_host_targets(function, &defs, "duplicate aggregate field `a`");
}

#[test]
fn aggregate_constructor_requires_exact_supplied_field_types() {
    let value = FirValueId(0);
    let function = aggregate_constructor(vec![("a".into(), value)], u(IntWidth::W32));
    let defs = BTreeMap::from([record_def()]);
    assert_invalid_on_host_targets(
        function,
        &defs,
        "make-aggregate field `a` has FIR payload type Int { signed: false, width: W32 }, declared field type is Int { signed: false, width: W8 }",
    );
}

#[test]
fn aggregate_constructor_requires_every_declared_field() {
    let value = FirValueId(0);
    let function = aggregate_constructor(vec![("a".into(), value)], u(IntWidth::W8));
    let defs = BTreeMap::from([record_def()]);
    assert_invalid_on_host_targets(
        function,
        &defs,
        "make-aggregate instruction is missing declared field `b`",
    );
}

#[test]
fn variant_cannot_construct_a_payload_bearing_tagged_variant() {
    let span = Span::new(0, 0);
    let tagged_ty = Ty::Nominal(DefId(101));
    let value = FirValueId(0);
    let function = FirFunction {
        owner: DefId(3),
        params: Vec::new(),
        return_type: tagged_ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span,
                result: Some(value),
                kind: FirInstructionKind::Variant {
                    ty: tagged_ty.clone(),
                    name: "Number".into(),
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, tagged_ty)]),
    };
    let defs = BTreeMap::from([tagged_def()]);
    assert_invalid_on_host_targets(
        function,
        &defs,
        "variant instruction cannot construct payload-bearing variant `Number`",
    );
}

#[test]
fn variant_test_requires_a_boolean_result() {
    let span = Span::new(0, 0);
    let tagged_ty = Ty::Nominal(DefId(101));
    let value = FirValueId(0);
    let result = FirValueId(1);
    let function = FirFunction {
        owner: DefId(4),
        params: Vec::new(),
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
                    result: Some(value),
                    kind: FirInstructionKind::Variant {
                        ty: tagged_ty.clone(),
                        name: "Empty".into(),
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::VariantIs {
                        value,
                        name: "Empty".into(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(value, tagged_ty), (result, u(IntWidth::W32))]),
    };
    let defs = BTreeMap::from([tagged_def()]);
    assert_invalid_on_host_targets(
        function,
        &defs,
        "variant-is instruction has non-bool FIR result type Int { signed: false, width: W32 }",
    );
}

#[test]
fn reordered_struct_constructs_and_extracts_field() {
    let span = Span::new(0, 0);
    let record_ty = Ty::Nominal(DefId(100));
    let values = [FirValueId(0), FirValueId(1), FirValueId(2), FirValueId(3)];
    let aggregate = FirValueId(4);
    let extracted = FirValueId(5);
    let function = FirFunction {
        owner: DefId(1),
        params: vec![],
        return_type: u(IntWidth::W64),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, values[0], "1"),
                scalar_const(span, values[1], "99"),
                scalar_const(span, values[2], "2"),
                scalar_const(span, values[3], "3"),
                FirInstruction {
                    span,
                    result: Some(aggregate),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: record_ty.clone(),
                        variant: None,
                        fields: vec![
                            ("a".into(), values[0]),
                            ("b".into(), values[1]),
                            ("c".into(), values[2]),
                            ("d".into(), values[3]),
                        ],
                    },
                },
                FirInstruction {
                    span,
                    result: Some(extracted),
                    kind: FirInstructionKind::ExtractField {
                        base: aggregate,
                        field: "b".into(),
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(extracted),
            }),
        }],
        value_types: BTreeMap::from([
            (values[0], u(IntWidth::W8)),
            (values[1], u(IntWidth::W64)),
            (values[2], u(IntWidth::W16)),
            (values[3], u(IntWidth::W32)),
            (aggregate, record_ty),
            (extracted, u(IntWidth::W64)),
        ]),
    };
    let defs = BTreeMap::from([record_def()]);
    assert_prepares(&module_with(function), &defs);
}

#[test]
fn aggregate_local_copy_and_field_address_use_layout_offsets() {
    let span = Span::new(0, 0);
    let record_ty = Ty::Nominal(DefId(100));
    let local = FirLocalId(0);
    let a = FirValueId(0);
    let b = FirValueId(1);
    let c = FirValueId(2);
    let d = FirValueId(3);
    let made = FirValueId(4);
    let loaded = FirValueId(5);
    let address = FirValueId(6);
    let result = FirValueId(7);
    let function = FirFunction {
        owner: DefId(2),
        params: vec![],
        return_type: u(IntWidth::W64),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: record_ty.clone(),
                mutable: true,
                parameter: false,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, a, "1"),
                scalar_const(span, b, "123"),
                scalar_const(span, c, "2"),
                scalar_const(span, d, "3"),
                FirInstruction {
                    span,
                    result: Some(made),
                    kind: FirInstructionKind::MakeAggregate {
                        ty: record_ty.clone(),
                        variant: None,
                        fields: vec![
                            ("a".into(), a),
                            ("b".into(), b),
                            ("c".into(), c),
                            ("d".into(), d),
                        ],
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local },
                        value: made,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::AddressOf {
                        place: FirPlace::Field {
                            base: Box::new(FirPlace::Local { local }),
                            field: "b".into(),
                        },
                        mutable: true,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([
            (a, u(IntWidth::W8)),
            (b, u(IntWidth::W64)),
            (c, u(IntWidth::W16)),
            (d, u(IntWidth::W32)),
            (made, record_ty.clone()),
            (loaded, record_ty),
            (
                address,
                Ty::Reference {
                    mutable: true,
                    inner: Box::new(u(IntWidth::W64)),
                },
            ),
            (result, u(IntWidth::W64)),
        ]),
    };
    let defs = BTreeMap::from([record_def()]);
    assert_prepares(&module_with(function), &defs);
}

#[test]
fn array_len_bounds_and_index_lower_together() {
    let span = Span::new(0, 0);
    let array_ty = Ty::Array {
        element: Box::new(u(IntWidth::W32)),
        length: Some(3),
    };
    let x = FirValueId(0);
    let y = FirValueId(1);
    let z = FirValueId(2);
    let array = FirValueId(3);
    let index = FirValueId(4);
    let len = FirValueId(5);
    let item = FirValueId(6);
    let usize_ty = u(IntWidth::Pointer);
    let function = FirFunction {
        owner: DefId(3),
        params: vec![],
        return_type: u(IntWidth::W32),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, x, "10"),
                scalar_const(span, y, "20"),
                scalar_const(span, z, "30"),
                FirInstruction {
                    span,
                    result: Some(array),
                    kind: FirInstructionKind::MakeArray {
                        items: vec![x, y, z],
                    },
                },
                scalar_const(span, index, "1"),
                FirInstruction {
                    span,
                    result: Some(len),
                    kind: FirInstructionKind::Len { value: array },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::BoundsCheck { index, len },
                },
                FirInstruction {
                    span,
                    result: Some(item),
                    kind: FirInstructionKind::IndexUnchecked { base: array, index },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(item) }),
        }],
        value_types: BTreeMap::from([
            (x, u(IntWidth::W32)),
            (y, u(IntWidth::W32)),
            (z, u(IntWidth::W32)),
            (array, array_ty),
            (index, usize_ty.clone()),
            (len, usize_ty),
            (item, u(IntWidth::W32)),
        ]),
    };
    assert_prepares(&module_with(function), &BTreeMap::new());
}

#[test]
fn option_reference_uses_null_niche_operations() {
    let span = Span::new(0, 0);
    let byte_local = FirLocalId(0);
    let reference_ty = Ty::Reference {
        mutable: false,
        inner: Box::new(Ty::Byte),
    };
    let option_ty = Ty::Optional {
        inner: Box::new(reference_ty.clone()),
    };
    let initial = FirValueId(0);
    let address = FirValueId(1);
    let some = FirValueId(2);
    let is_some = FirValueId(3);
    let function = FirFunction {
        owner: DefId(4),
        params: vec![],
        return_type: Ty::Bool,
        locals: BTreeMap::from([(
            byte_local,
            FirLocal {
                id: byte_local,
                source: None,
                ty: Ty::Byte,
                mutable: false,
                parameter: false,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, initial, "0"),
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local: byte_local },
                        value: initial,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::AddressOf {
                        place: FirPlace::Local { local: byte_local },
                        mutable: false,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(some),
                    kind: FirInstructionKind::MakeSome { value: address },
                },
                FirInstruction {
                    span,
                    result: Some(is_some),
                    kind: FirInstructionKind::OptionIsSome { value: some },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(is_some),
            }),
        }],
        value_types: BTreeMap::from([
            (initial, Ty::Byte),
            (address, reference_ty),
            (some, option_ty),
            (is_some, Ty::Bool),
        ]),
    };
    assert_prepares(&module_with(function), &BTreeMap::new());
}

#[test]
fn explicit_result_tag_is_written_and_tested() {
    let span = Span::new(0, 0);
    let result_ty = Ty::Result {
        ok: Box::new(u(IntWidth::W32)),
        error: Box::new(u(IntWidth::W32)),
    };
    let error = FirValueId(0);
    let result = FirValueId(1);
    let is_ok = FirValueId(2);
    let function = FirFunction {
        owner: DefId(5),
        params: vec![],
        return_type: Ty::Bool,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                scalar_const(span, error, "7"),
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::MakeResultErr { error },
                },
                FirInstruction {
                    span,
                    result: Some(is_ok),
                    kind: FirInstructionKind::ResultIsOk { value: result },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(is_ok) }),
        }],
        value_types: BTreeMap::from([
            (error, u(IntWidth::W32)),
            (result, result_ty),
            (is_ok, Ty::Bool),
        ]),
    };
    assert_prepares(&module_with(function), &BTreeMap::new());
}
