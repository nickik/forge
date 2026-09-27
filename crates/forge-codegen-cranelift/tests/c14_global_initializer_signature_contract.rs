use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirGlobal, FirGlobalInitializer, FirLocal,
    FirLocalId, FirModule, FirTerminator, IntWidth, Ty, TypeDefinitionTable,
};

fn parameterized_initializer(owner: DefId) -> FirFunction {
    let parameter = FirLocalId(0);
    FirFunction {
        owner,
        params: vec![parameter],
        return_type: Ty::Void,
        locals: BTreeMap::from([(
            parameter,
            FirLocal {
                id: parameter,
                source: None,
                ty: Ty::Int {
                    signed: false,
                    width: IntWidth::W32,
                },
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
            instructions: Vec::new(),
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::new(),
    }
}

#[test]
fn native_backends_reject_parameterized_global_initializers() {
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
                function: parameterized_initializer(owner),
            },
        )]),
        global_init_order: vec![owner],
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("parameterized global initializer unexpectedly lowered"),
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
