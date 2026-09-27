use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    CaptureMode, DefId, ExprId, FirBasicBlock, FirBlockId, FirClosure, FirClosureField,
    FirFunction, FirModule, FirTerminator, IntWidth, LocalId, Ty, TypeDefinitionTable,
};

fn malformed_orphan_closure() -> FirFunction {
    let closure = ExprId(7);
    FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type: Ty::Void,
        locals: BTreeMap::new(),
        closures: BTreeMap::from([(
            closure,
            FirClosure {
                id: closure,
                captures: vec![FirClosureField {
                    local: LocalId(0),
                    ty: Ty::Int {
                        signed: false,
                        width: IntWidth::W32,
                    },
                    mode: CaptureMode::Value,
                }],
                params: Vec::new(),
                return_type: Ty::Void,
                entry: FirBlockId(1),
                function_pointer: true,
            },
        )]),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: Vec::new(),
                terminator: Some(FirTerminator::Return { value: None }),
            },
            FirBasicBlock {
                id: FirBlockId(1),
                closure: Some(closure),
                instructions: Vec::new(),
                terminator: Some(FirTerminator::Return { value: None }),
            },
        ],
        value_types: BTreeMap::new(),
    }
}

#[test]
fn native_backends_reject_malformed_orphan_closure_metadata() {
    let function = malformed_orphan_closure();
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed orphan closure metadata unexpectedly lowered"),
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
