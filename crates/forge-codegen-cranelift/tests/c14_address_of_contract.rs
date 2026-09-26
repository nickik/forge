use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable, UnsafeProvenance,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn reference(inner: Ty, mutable: bool) -> Ty {
    Ty::Reference {
        mutable,
        inner: Box::new(inner),
    }
}

fn pointer(inner: Ty) -> Ty {
    Ty::Pointer {
        volatile: false,
        inner: Box::new(inner),
    }
}

fn address_of(local_mutable: bool, requested_mutable: bool, result_ty: Ty) -> FirFunction {
    let local_id = FirLocalId(0);
    let result = FirValueId(0);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![local_id],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            local_id,
            FirLocal {
                id: local_id,
                source: None,
                ty: u32_ty(),
                mutable: local_mutable,
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
                result: Some(result),
                kind: FirInstructionKind::AddressOf {
                    place: FirPlace::Local { local: local_id },
                    mutable: requested_mutable,
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(result, result_ty)]),
    }
}

fn dereference_address_of(
    raw: bool,
    source_ty: Ty,
    requested_mutable: bool,
    result_ty: Ty,
) -> FirFunction {
    let local_id = FirLocalId(0);
    let address = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    let place = if raw {
        FirPlace::RawDeref {
            address,
            volatile: false,
            provenance: UnsafeProvenance { scope: span },
        }
    } else {
        FirPlace::Deref { address }
    };
    FirFunction {
        owner: DefId(1),
        params: vec![local_id],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            local_id,
            FirLocal {
                id: local_id,
                source: None,
                ty: source_ty.clone(),
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
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(address),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: local_id },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::AddressOf {
                        place,
                        mutable: requested_mutable,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(address, source_ty), (result, result_ty)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed address-of FIR unexpectedly lowered"),
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
fn address_of_result_must_match_the_local_pointee() {
    assert_invalid(
        address_of(false, false, reference(Ty::Byte, false)),
        "address-of result type Reference { mutable: false, inner: Byte } does not match expected reference type Reference { mutable: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn address_of_result_must_preserve_requested_mutability() {
    assert_invalid(
        address_of(true, true, reference(u32_ty(), false)),
        "address-of result type Reference { mutable: false, inner: Int { signed: false, width: W32 } } does not match expected reference type Reference { mutable: true, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn mutable_address_requires_a_mutable_local() {
    assert_invalid(
        address_of(false, true, reference(u32_ty(), true)),
        "mutable address requested for immutable FIR local FirLocalId(0)",
    );
}

#[test]
fn safe_dereference_address_of_result_must_match_the_reference_pointee() {
    assert_invalid(
        dereference_address_of(
            false,
            reference(u32_ty(), false),
            false,
            reference(Ty::Byte, false),
        ),
        "address-of result type Reference { mutable: false, inner: Byte } does not match expected reference type Reference { mutable: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn raw_dereference_address_of_result_must_match_the_pointer_pointee() {
    assert_invalid(
        dereference_address_of(
            true,
            pointer(u32_ty()),
            false,
            reference(Ty::Byte, false),
        ),
        "address-of result type Reference { mutable: false, inner: Byte } does not match expected reference type Reference { mutable: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn dereference_address_of_result_must_preserve_requested_mutability() {
    assert_invalid(
        dereference_address_of(
            false,
            reference(u32_ty(), true),
            true,
            reference(u32_ty(), false),
        ),
        "address-of result type Reference { mutable: false, inner: Int { signed: false, width: W32 } } does not match expected reference type Reference { mutable: true, inner: Int { signed: false, width: W32 } }",
    );
}
