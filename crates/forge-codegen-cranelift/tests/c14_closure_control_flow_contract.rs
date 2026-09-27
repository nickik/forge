use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, ExprId, FirBasicBlock, FirBlockId, FirClosure, FirFunction, FirModule, FirTerminator,
    Ty, TypeDefinitionTable,
};

fn crossing_function() -> FirFunction {
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
                captures: Vec::new(),
                params: Vec::new(),
                return_type: Ty::Void,
                entry: FirBlockId(1),
                function_pointer: false,
            },
        )]),
        entry: FirBlockId(0),
        blocks: vec![
            FirBasicBlock {
                id: FirBlockId(0),
                closure: None,
                instructions: Vec::new(),
                terminator: Some(FirTerminator::Goto {
                    target: FirBlockId(1),
                }),
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
fn native_backends_reject_control_flow_between_closure_bodies() {
    let function = crossing_function();
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("cross-body closure control flow unexpectedly lowered"),
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
