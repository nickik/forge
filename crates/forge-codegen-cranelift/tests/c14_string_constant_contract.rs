use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty, TypeDefinitionTable,
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
