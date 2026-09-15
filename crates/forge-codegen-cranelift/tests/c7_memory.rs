use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    UnsafeProvenance,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn usize_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    }
}

fn pointer(inner: Ty) -> Ty {
    Ty::Pointer {
        volatile: false,
        inner: Box::new(inner),
    }
}

fn reference(inner: Ty, mutable: bool) -> Ty {
    Ty::Reference {
        mutable,
        inner: Box::new(inner),
    }
}

fn local(id: u32, ty: Ty, mutable: bool, parameter: bool) -> (FirLocalId, FirLocal) {
    let id = FirLocalId(id);
    (
        id,
        FirLocal {
            id,
            source: None,
            ty,
            mutable,
            parameter,
            synthetic: false,
        },
    )
}

fn lower(function: FirFunction, target: CraneliftTarget) -> Result<String, BackendError> {
    let owner = function.owner;
    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    let backend = CraneliftBackend::new(target)?;
    let prepared = backend.prepare_module(&module)?;
    let clif = prepared
        .function(owner)
        .expect("prepared function")
        .display()
        .to_string();
    let machine = backend.emit_machine_code(&prepared, owner)?;
    assert!(!machine.bytes().is_empty());
    Ok(clif)
}

fn targets() -> [CraneliftTarget; 2] {
    [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64]
}

#[test]
fn local_store_and_load_use_scalar_stack_storage_on_both_targets() {
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let (local_id, local_data) = local(0, ty.clone(), true, false);
    let constant = FirValueId(0);
    let loaded = FirValueId(1);
    let owner = DefId(0);

    let function = FirFunction {
        owner,
        params: vec![],
        return_type: ty.clone(),
        locals: BTreeMap::from([(local_id, local_data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(constant),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "42".into() },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local: local_id },
                        value: constant,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: local_id },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([(constant, ty.clone()), (loaded, ty)]),
    };

    for target in targets() {
        let clif = lower(function.clone(), target).expect("C7 local memory lowering");
        assert!(clif.contains("explicit_slot 8"), "{clif}");
        assert!(clif.contains("stack_addr"), "{clif}");
        assert!(clif.contains("store"), "{clif}");
        assert!(clif.contains("load.i64"), "{clif}");
    }
}

#[test]
fn address_of_local_and_safe_reference_deref_lower_on_both_targets() {
    let span = Span::new(0, 0);
    let value_ty = u32_ty();
    let ref_ty = reference(value_ty.clone(), false);
    let (local_id, local_data) = local(0, value_ty.clone(), true, false);
    let constant = FirValueId(0);
    let address = FirValueId(1);
    let loaded = FirValueId(2);
    let owner = DefId(1);

    let function = FirFunction {
        owner,
        params: vec![],
        return_type: value_ty.clone(),
        locals: BTreeMap::from([(local_id, local_data)]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(constant),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "7".into() },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local: local_id },
                        value: constant,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::AddressOf {
                        place: FirPlace::Local { local: local_id },
                        mutable: false,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([
            (constant, value_ty.clone()),
            (address, ref_ty),
            (loaded, value_ty),
        ]),
    };

    for target in targets() {
        let clif = lower(function.clone(), target).expect("C7 reference lowering");
        assert!(clif.contains("explicit_slot 4"), "{clif}");
        assert!(clif.contains("stack_addr"), "{clif}");
        assert!(clif.contains("load.i32"), "{clif}");
    }
}

#[test]
fn raw_pointer_load_and_store_lower_and_codegen_on_both_targets() {
    let span = Span::new(0, 0);
    let value_ty = u64_ty();
    let pointer_ty = pointer(value_ty.clone());
    let (pointer_local, pointer_data) = local(0, pointer_ty.clone(), false, true);
    let (value_local, value_data) = local(1, value_ty.clone(), false, true);
    let pointer_value = FirValueId(0);
    let input_value = FirValueId(1);
    let loaded = FirValueId(2);
    let owner = DefId(2);
    let provenance = UnsafeProvenance { scope: span };

    let function = FirFunction {
        owner,
        params: vec![pointer_local, value_local],
        return_type: value_ty.clone(),
        locals: BTreeMap::from([
            (pointer_local, pointer_data),
            (value_local, value_data),
        ]),
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
                    result: Some(input_value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: value_local },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::RawDeref {
                            address: pointer_value,
                            volatile: false,
                            provenance,
                        },
                        value: input_value,
                    },
                },
                FirInstruction {
                    span,
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::RawDeref {
                            address: pointer_value,
                            volatile: false,
                            provenance,
                        },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([
            (pointer_value, pointer_ty),
            (input_value, value_ty.clone()),
            (loaded, value_ty),
        ]),
    };

    for target in targets() {
        let clif = lower(function.clone(), target).expect("C7 raw pointer lowering");
        assert!(clif.matches("store").count() >= 3, "{clif}");
        assert!(clif.matches("load.i64").count() >= 3, "{clif}");
    }
}

#[test]
fn pointer_offset_scales_by_forge_pointee_layout() {
    let span = Span::new(0, 0);
    let element_ty = u32_ty();
    let pointer_ty = pointer(element_ty);
    let offset_ty = usize_ty();
    let (pointer_local, pointer_data) = local(0, pointer_ty.clone(), false, true);
    let (offset_local, offset_data) = local(1, offset_ty.clone(), false, true);
    let pointer_value = FirValueId(0);
    let offset_value = FirValueId(1);
    let result = FirValueId(2);
    let owner = DefId(3);

    let function = FirFunction {
        owner,
        params: vec![pointer_local, offset_local],
        return_type: pointer_ty.clone(),
        locals: BTreeMap::from([
            (pointer_local, pointer_data),
            (offset_local, offset_data),
        ]),
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
                        place: FirPlace::Local { local: offset_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::PointerOffset {
                        pointer: pointer_value,
                        offset: offset_value,
                        subtract: false,
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

    for target in targets() {
        let clif = lower(function.clone(), target).expect("C7 pointer offset lowering");
        assert!(clif.contains("iconst.i64 4"), "{clif}");
        assert!(clif.contains("imul"), "{clif}");
        assert!(clif.contains("iadd"), "{clif}");
    }
}

#[test]
fn volatile_raw_dereference_remains_an_explicit_boundary() {
    let span = Span::new(0, 0);
    let value_ty = u64_ty();
    let pointer_ty = pointer(value_ty.clone());
    let (pointer_local, pointer_data) = local(0, pointer_ty.clone(), false, true);
    let pointer_value = FirValueId(0);
    let loaded = FirValueId(1);
    let owner = DefId(4);

    let function = FirFunction {
        owner,
        params: vec![pointer_local],
        return_type: value_ty.clone(),
        locals: BTreeMap::from([(pointer_local, pointer_data)]),
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
                    result: Some(loaded),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::RawDeref {
                            address: pointer_value,
                            volatile: true,
                            provenance: UnsafeProvenance { scope: span },
                        },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(loaded),
            }),
        }],
        value_types: BTreeMap::from([
            (pointer_value, pointer_ty),
            (loaded, value_ty),
        ]),
    };

    let error = lower(function, CraneliftTarget::Aarch64)
        .expect_err("volatile semantics must not silently become an ordinary load");
    assert_eq!(
        error,
        BackendError::UnsupportedInstruction {
            kind: "volatile raw dereference"
        }
    );
}
