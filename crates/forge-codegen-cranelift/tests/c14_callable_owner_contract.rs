use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirGlobal, FirGlobalInitializer, FirModule,
    FirTerminator, Ty, TypeDefinitionTable,
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

fn assert_rejected(module: FirModule) {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed FIR callable ownership unexpectedly lowered"),
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

#[test]
fn native_backends_reject_mismatched_function_map_owners() {
    let module = FirModule {
        functions: BTreeMap::from([(DefId(1), void_function(DefId(2)))]),
        ..FirModule::default()
    };
    assert_rejected(module);
}

#[test]
fn native_backends_reject_mismatched_initializer_function_owners() {
    let owner = DefId(1);
    let module = FirModule {
        globals: BTreeMap::from([(
            owner,
            FirGlobal {
                owner,
                ty: Ty::Void,
                mutable: false,
                constant: None,
            },
        )]),
        global_initializers: BTreeMap::from([(
            owner,
            FirGlobalInitializer {
                owner,
                dependencies: Vec::new(),
                function: void_function(DefId(2)),
            },
        )]),
        global_init_order: vec![owner],
        ..FirModule::default()
    };
    assert_rejected(module);
}
