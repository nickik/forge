use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal, FirInstruction,
    FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span,
    StaticGlobalInitializer, StaticGlobalInitializerTable, StaticValue, Ty, TypeDefinitionTable,
};

fn function(result_ty: Option<Ty>) -> FirFunction {
    let result = FirValueId(0);
    let result_id = result_ty.as_ref().map(|_| result);
    let value_types = result_ty
        .clone()
        .map(|ty| BTreeMap::from([(result, ty)]))
        .unwrap_or_default();
    FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type: result_ty.unwrap_or(Ty::Void),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: result_id,
                kind: FirInstructionKind::Const {
                    value: FirConst::String {
                        value: "name".into(),
                    },
                },
            }],
            terminator: Some(FirTerminator::Return { value: result_id }),
        }],
        value_types,
    }
}

fn c_string_function(result_ty: Option<Ty>) -> FirFunction {
    let result = FirValueId(0);
    let result_id = result_ty.as_ref().map(|_| result);
    let value_types = result_ty
        .clone()
        .map(|ty| BTreeMap::from([(result, ty)]))
        .unwrap_or_default();
    FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type: result_ty.unwrap_or(Ty::Void),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: result_id,
                kind: FirInstructionKind::Const {
                    value: FirConst::CString {
                        value: "name".into(),
                    },
                },
            }],
            terminator: Some(FirTerminator::Return { value: result_id }),
        }],
        value_types,
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed string constant FIR unexpectedly lowered"),
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

#[test]
fn string_constant_requires_a_result() {
    assert_invalid(function(None), "string constant has no result");
}

#[test]
fn string_constant_requires_a_str_result() {
    assert_invalid(
        function(Some(Ty::Int {
            signed: false,
            width: IntWidth::W32,
        })),
        "string constant has non-str FIR result type Int { signed: false, width: W32 }",
    );
}

#[test]
fn c_string_constant_requires_a_result() {
    assert_invalid(c_string_function(None), "C string constant has no result");
}

#[test]
fn c_string_constant_requires_a_byte_pointer_result() {
    assert_invalid(
        c_string_function(Some(Ty::Int {
            signed: false,
            width: IntWidth::W32,
        })),
        "C string constant has non-byte-pointer FIR result type Int { signed: false, width: W32 }",
    );
}

#[test]
fn c_string_constant_pins_static_data_lowering_boundary() {
    let mut module = FirModule::default();
    let function = c_string_function(Some(Ty::Pointer {
        volatile: false,
        inner: Box::new(Ty::Byte),
    }));
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("C string constant unexpectedly lowered without static data"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::UnsupportedInstruction {
                kind: "C string literal requires static-data lowering",
            }
        );
    }
}

#[test]
fn dedicated_static_data_address_lowers_for_immutable_nul_terminated_bytes() {
    let function_owner = DefId(1);
    let storage_owner = DefId(2);
    let result = FirValueId(0);
    let pointer_ty = Ty::Pointer {
        volatile: false,
        inner: Box::new(Ty::Byte),
    };
    let function = FirFunction {
        owner: function_owner,
        params: Vec::new(),
        return_type: pointer_ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(result),
                kind: FirInstructionKind::StaticDataAddress {
                    global: storage_owner,
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(result, pointer_ty)]),
    };
    let mut module = FirModule::default();
    module.functions.insert(function_owner, function);
    module.globals.insert(
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
    let byte = |value| StaticValue::Scalar(ConstValue::Integer { value });
    let initializers = StaticGlobalInitializerTable::from([(
        storage_owner,
        StaticGlobalInitializer {
            value: StaticValue::Array(vec![byte(110), byte(97), byte(109), byte(101), byte(0)]),
            writable: false,
        },
    )]);

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module_with_static_initializers(
                &module,
                &TypeDefinitionTable::new(),
                &initializers,
            )
            .expect("static-data address should lower");
        assert_eq!(
            prepared
                .global(storage_owner)
                .expect("literal storage")
                .static_data()
                .expect("literal bytes")
                .bytes(),
            b"name\0"
        );
        let first = backend
            .emit_object_with_exports(&prepared, [function_owner])
            .expect("object")
            .into_bytes();
        let second = backend
            .emit_object_with_exports(&prepared, [function_owner])
            .expect("deterministic object")
            .into_bytes();
        assert_eq!(first, second, "{target:?} object must be deterministic");
        assert_eq!(&first[..4], b"\x7fELF");
    }

    let mut malformed = module.clone();
    malformed
        .globals
        .get_mut(&storage_owner)
        .expect("literal storage")
        .mutable = true;
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_static_initializers(
            &malformed,
            &TypeDefinitionTable::new(),
            &initializers,
        ) {
            Ok(_) => panic!("mutable static-data address storage unexpectedly verified"),
            Err(error) => error,
        };
        assert_eq!(
            error,
            BackendError::InvalidFir {
                diagnostic_count: 1,
            }
        );
    }
}
