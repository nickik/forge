use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirFunction, FirGlobal, FirModule, FirTerminator,
    Ty, TypeDefinitionTable,
};

fn void_function(owner: DefId) -> FirFunction {
    FirFunction {
        owner,
        params: Vec::new(),
        return_type: Ty::Void,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: Vec::new(),
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::new(),
    }
}

#[test]
fn native_backends_reject_function_global_identity_collisions() {
    let owner = DefId(1);
    let module = FirModule {
        functions: BTreeMap::from([(owner, void_function(owner))]),
        globals: BTreeMap::from([(
            owner,
            FirGlobal {
                owner,
                ty: Ty::Bool,
                mutable: false,
                constant: Some(ConstValue::Bool { value: true }),
            },
        )]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("function/global identity collision unexpectedly lowered"),
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
