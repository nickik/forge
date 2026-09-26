use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn local(id: FirLocalId, ty: Ty) -> FirLocal {
    FirLocal {
        id,
        source: None,
        ty,
        mutable: false,
        parameter: true,
        synthetic: false,
    }
}

fn mismatched_local_store() -> FirFunction {
    let target = FirLocalId(0);
    let source = FirLocalId(1);
    let value = FirValueId(0);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![target, source],
        return_type: Ty::Void,
        locals: BTreeMap::from([
            (target, local(target, u32_ty())),
            (source, local(source, Ty::Byte)),
        ]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(value),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: source },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::Store {
                        place: FirPlace::Local { local: target },
                        value,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(value, Ty::Byte)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed local-store FIR unexpectedly lowered"),
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
fn local_store_value_must_match_the_local_type() {
    assert_invalid(
        mismatched_local_store(),
        "local FIR store value type Byte differs from local type Int { signed: false, width: W32 }",
    );
}
