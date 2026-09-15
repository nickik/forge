use std::collections::BTreeMap;

use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, ScalarLayout, TargetLayout, TypeLowering,
};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    UnsafeProvenance,
};

fn int(signed: bool, width: IntWidth) -> Ty {
    Ty::Int { signed, width }
}

fn pointer(inner: Ty) -> Ty {
    Ty::Pointer {
        volatile: false,
        inner: Box::new(inner),
    }
}

fn local(id: u32, ty: Ty, parameter: bool) -> (FirLocalId, FirLocal) {
    let id = FirLocalId(id);
    (
        id,
        FirLocal {
            id,
            source: None,
            ty,
            mutable: false,
            parameter,
            synthetic: false,
        },
    )
}

fn prepare(function: FirFunction, target: CraneliftTarget) -> Result<String, BackendError> {
    let owner = function.owner;
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    let backend = CraneliftBackend::new(target)?;
    let prepared = backend.prepare_module(&module)?;
    let clif = prepared
        .function(owner)
        .expect("prepared C7 function")
        .display()
        .to_string();
    assert!(!backend
        .emit_machine_code(&prepared, owner)?
        .bytes()
        .is_empty());
    Ok(clif)
}

#[test]
fn forge_scalar_layout_is_explicit_and_target_driven() {
    let layout64 = TargetLayout::new(64);
    let lowering64 = TypeLowering::new(&layout64);
    let cases = [
        (Ty::Bool, ScalarLayout::new(1, 1)),
        (Ty::Byte, ScalarLayout::new(1, 1)),
        (int(true, IntWidth::W8), ScalarLayout::new(1, 1)),
        (int(false, IntWidth::W16), ScalarLayout::new(2, 2)),
        (int(true, IntWidth::W32), ScalarLayout::new(4, 4)),
        (int(false, IntWidth::W64), ScalarLayout::new(8, 8)),
        (int(false, IntWidth::Pointer), ScalarLayout::new(8, 8)),
        (pointer(Ty::Bool), ScalarLayout::new(8, 8)),
        (
            Ty::Reference {
                mutable: true,
                inner: Box::new(int(false, IntWidth::W32)),
            },
            ScalarLayout::new(8, 8),
        ),
        (
            Ty::Function {
                params: vec![Ty::Bool],
                result: Box::new(Ty::Bool),
                named_arguments: false,
            },
            ScalarLayout::new(8, 8),
        ),
    ];

    for (ty, expected) in cases {
        assert_eq!(
            lowering64.scalar_layout(&ty),
            Ok(expected),
            "layout for {ty:?}"
        );
    }

    let layout32 = TargetLayout::new(32);
    let lowering32 = TypeLowering::new(&layout32);
    for ty in [
        int(false, IntWidth::Pointer),
        pointer(Ty::Bool),
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Bool),
        },
    ] {
        assert_eq!(
            lowering32.scalar_layout(&ty),
            Ok(ScalarLayout::new(4, 4)),
            "32-bit pointer layout for {ty:?}"
        );
    }

    let aggregate = Ty::Array {
        element: Box::new(Ty::Byte),
        length: Some(4),
    };
    assert_eq!(
        lowering64.scalar_layout(&aggregate),
        Err(BackendError::UnsupportedType { kind: "array" })
    );
}

#[test]
fn cross_block_raw_deref_dependency_is_scheduled_before_use() {
    let span = Span::new(0, 0);
    let value_ty = int(false, IntWidth::W64);
    let pointer_ty = pointer(value_ty.clone());
    let (param, param_data) = local(0, pointer_ty.clone(), true);
    let pointer_value = FirValueId(0);
    let loaded = FirValueId(1);
    let owner = DefId(20);

    // Block IDs remain vector indices. Control flow is 0 -> 2 -> 1 so the
    // pointer definition in block 2 dominates the raw dereference in block 1,
    // despite block 1 appearing first in the vector.
    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: value_ty.clone(),
        locals: BTreeMap::from([(param, param_data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: vec![],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(2),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::RawDeref {
                            address: pointer_value,
                            volatile: false,
                            provenance: UnsafeProvenance { scope: span },
                        },
                    },
                }],
                terminator: Some(FirTerminator::Return {
                    value: Some(loaded),
                }),
            },
            FirBasicBlock {
                id: FirBlockId(2),
                closure: None,
                instructions: vec![FirInstruction {
                    span,
                    result: Some(pointer_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                }],
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(1),
                }),
            },
        ],
        value_types: BTreeMap::from([(pointer_value, pointer_ty), (loaded, value_ty)]),
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let clif = prepare(function.clone(), target).expect("scheduled C7 raw dereference");
        assert!(clif.contains("load.i64"), "{clif}");
    }
}

#[test]
fn pointer_subtraction_uses_forge_pointee_stride() {
    let span = Span::new(0, 0);
    let element_ty = int(false, IntWidth::W32);
    let pointer_ty = pointer(element_ty);
    let offset_ty = int(false, IntWidth::Pointer);
    let (pointer_local, pointer_data) = local(0, pointer_ty.clone(), true);
    let (offset_local, offset_data) = local(1, offset_ty.clone(), true);
    let pointer_value = FirValueId(0);
    let offset_value = FirValueId(1);
    let result = FirValueId(2);
    let owner = DefId(21);

    let function = FirFunction {
        owner,
        params: vec![pointer_local, offset_local],
        return_type: pointer_ty.clone(),
        locals: BTreeMap::from([(pointer_local, pointer_data), (offset_local, offset_data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(pointer_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: pointer_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(offset_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: offset_local,
                        },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::PointerOffset {
                        pointer: pointer_value,
                        offset: offset_value,
                        subtract: true,
                        provenance: UnsafeProvenance { scope: span },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([
            (pointer_value, pointer_ty.clone()),
            (offset_value, offset_ty),
            (result, pointer_ty),
        ]),
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let clif = prepare(function.clone(), target).expect("C7 pointer subtraction");
        assert!(clif.contains("iconst.i64 4"), "{clif}");
        assert!(clif.contains("imul"), "{clif}");
        assert!(clif.contains("isub"), "{clif}");
    }
}
