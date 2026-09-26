use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn function(source_ty: Ty, result_ty: Ty) -> FirFunction {
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
                    kind: FirInstructionKind::Len { value: source },
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
            Ok(_) => panic!("malformed len FIR unexpectedly lowered"),
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
fn len_requires_a_sequence_input() {
    assert_invalid(
        function(u(IntWidth::W32), u(IntWidth::Pointer)),
        "len instruction has unsupported FIR input type Int { signed: false, width: W32 }",
    );
}

#[test]
fn len_rejects_unsized_array_inputs() {
    assert_invalid(
        function(
            Ty::Array {
                element: Box::new(u(IntWidth::W16)),
                length: None,
            },
            u(IntWidth::Pointer),
        ),
        "len instruction has unsupported FIR input type Array { element: Int { signed: false, width: W16 }, length: None }",
    );
}

#[test]
fn len_requires_a_usize_result() {
    assert_invalid(
        function(
            Ty::Slice {
                mutable: false,
                element: Box::new(u(IntWidth::W16)),
            },
            u(IntWidth::W32),
        ),
        "len instruction has non-usize FIR result type Int { signed: false, width: W32 }",
    );
}
