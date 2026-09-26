use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty, TypeDefinition,
    TypeDefinitionKind, TypeDefinitionTable,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn definitions() -> TypeDefinitionTable {
    let owner = DefId(100);
    BTreeMap::from([(
        owner,
        TypeDefinition {
            owner,
            kind: TypeDefinitionKind::BitStruct {
                storage: u(IntWidth::W16),
            },
        },
    )])
}

fn function(
    source_ty: Ty,
    result_ty: Option<Ty>,
    instruction: impl FnOnce(FirValueId) -> FirInstructionKind,
) -> FirFunction {
    let local = FirLocalId(0);
    let source = FirValueId(0);
    let result = result_ty.as_ref().map(|_| FirValueId(1));
    let span = Span::new(0, 0);
    let mut value_types = BTreeMap::from([(source, source_ty.clone())]);
    if let (Some(result), Some(result_ty)) = (result, result_ty.as_ref()) {
        value_types.insert(result, result_ty.clone());
    }
    FirFunction {
        owner: DefId(1),
        params: vec![local],
        return_type: result_ty.clone().unwrap_or(Ty::Void),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: source_ty,
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
                    result,
                    kind: instruction(source),
                },
            ],
            terminator: Some(FirTerminator::Return { value: result }),
        }],
        value_types,
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = definitions();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed bitstruct FIR unexpectedly lowered"),
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
fn bitstruct_storage_projection_requires_declared_storage() {
    assert_invalid(
        function(
            Ty::Nominal(DefId(100)),
            Some(u(IntWidth::W32)),
            |value| FirInstructionKind::BitStructStorage {
                value,
                storage: u(IntWidth::W32),
            },
        ),
        "bitstruct storage projection uses the wrong storage type",
    );
}

#[test]
fn bitstruct_rebuild_requires_declared_storage() {
    assert_invalid(
        function(u(IntWidth::W32), Some(Ty::Nominal(DefId(100))), |value| {
            FirInstructionKind::BitStructFromStorage {
                value,
                bitstruct: DefId(100),
            }
        }),
        "bitstruct rebuild input differs from storage type",
    );
}

#[test]
fn bitfield_range_check_requires_a_strictly_narrower_width() {
    assert_invalid(
        function(u(IntWidth::W8), None, |value| {
            FirInstructionKind::BitFieldCheck { value, width: 8 }
        }),
        "bitfield range check has an invalid field width",
    );
}

#[test]
fn bitfield_extract_cannot_widen_storage() {
    assert_invalid(
        function(u(IntWidth::W8), Some(u(IntWidth::W16)), |value| {
            FirInstructionKind::BitFieldExtract { value }
        }),
        "bitfield extract widens its storage value",
    );
}

#[test]
fn bitfield_extend_cannot_narrow_payload() {
    assert_invalid(
        function(u(IntWidth::W16), Some(u(IntWidth::W8)), |value| {
            FirInstructionKind::BitFieldExtend { value }
        }),
        "bitfield extend narrows its field value",
    );
}
