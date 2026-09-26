use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable, UnsafeOperationKind, UnsafeProvenance,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn pointer(inner: Ty) -> Ty {
    Ty::Pointer {
        volatile: false,
        inner: Box::new(inner),
    }
}

fn function(
    source_ty: Ty,
    target: Ty,
    operation: UnsafeOperationKind,
    result_ty: Ty,
) -> FirFunction {
    let local = FirLocalId(0);
    let source = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
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
                    result: Some(source),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::PointerConvert {
                        value: source,
                        target,
                        operation,
                        provenance: UnsafeProvenance { scope: span },
                    },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(source, source_ty), (result, result_ty)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed pointer conversion FIR unexpectedly lowered"),
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
fn pointer_convert_target_must_match_its_result() {
    assert_invalid(
        function(
            pointer(u(IntWidth::W32)),
            u(IntWidth::Pointer),
            UnsafeOperationKind::PointerToInteger,
            u(IntWidth::W32),
        ),
        "pointer-convert target Int { signed: false, width: Pointer } does not match FIR result type Int { signed: false, width: W32 }",
    );
}

#[test]
fn pointer_to_integer_tag_requires_pointer_and_integer_endpoints() {
    assert_invalid(
        function(
            u(IntWidth::W32),
            u(IntWidth::Pointer),
            UnsafeOperationKind::PointerToInteger,
            u(IntWidth::Pointer),
        ),
        "pointer-convert operation PointerToInteger is incompatible with FIR types Int { signed: false, width: W32 } -> Int { signed: false, width: Pointer }",
    );
}

#[test]
fn integer_to_pointer_tag_requires_integer_and_pointer_endpoints() {
    let pointer_ty = pointer(u(IntWidth::W32));
    assert_invalid(
        function(
            pointer_ty.clone(),
            pointer_ty.clone(),
            UnsafeOperationKind::IntegerToPointer,
            pointer_ty,
        ),
        "pointer-convert operation IntegerToPointer is incompatible with FIR types Pointer { volatile: false, inner: Int { signed: false, width: W32 } } -> Pointer { volatile: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn pointer_reinterpret_tag_requires_distinct_pointer_types() {
    let pointer_ty = pointer(u(IntWidth::W32));
    assert_invalid(
        function(
            pointer_ty.clone(),
            pointer_ty.clone(),
            UnsafeOperationKind::PointerReinterpret,
            pointer_ty,
        ),
        "pointer-convert operation PointerReinterpret is incompatible with FIR types Pointer { volatile: false, inner: Int { signed: false, width: W32 } } -> Pointer { volatile: false, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn pointer_convert_rejects_non_conversion_operation_tags() {
    assert_invalid(
        function(
            pointer(u(IntWidth::W32)),
            u(IntWidth::Pointer),
            UnsafeOperationKind::PointerOffset { subtract: false },
            u(IntWidth::Pointer),
        ),
        "pointer-convert instruction uses non-conversion operation PointerOffset { subtract: false }",
    );
}
