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

fn function(source_ty: Ty, result_ty: Ty, items: Vec<FirValueId>) -> FirFunction {
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
                    kind: FirInstructionKind::MakeArray { items },
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
            Ok(_) => panic!("malformed make-array FIR unexpectedly lowered"),
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
fn make_array_requires_a_fixed_array_result() {
    assert_invalid(
        function(u(IntWidth::W16), u(IntWidth::W16), vec![FirValueId(0)]),
        "make-array instruction has non-fixed-array FIR result type Int { signed: false, width: W16 }",
    );
}

#[test]
fn make_array_requires_the_declared_length() {
    assert_invalid(
        function(
            u(IntWidth::W16),
            Ty::Array {
                element: Box::new(u(IntWidth::W16)),
                length: Some(2),
            },
            vec![FirValueId(0)],
        ),
        "make-array instruction declares length 2, but has 1 item(s)",
    );
}

#[test]
fn make_array_requires_exact_element_types() {
    assert_invalid(
        function(
            u(IntWidth::W32),
            Ty::Array {
                element: Box::new(u(IntWidth::W16)),
                length: Some(1),
            },
            vec![FirValueId(0)],
        ),
        "make-array item has FIR type Int { signed: false, width: W32 }, array element type is Int { signed: false, width: W16 }",
    );
}
