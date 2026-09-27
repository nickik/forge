use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirLocal, FirLocalId, FirModule, FirTerminator,
    IntWidth, Ty, TypeDefinitionTable,
};

fn parameter_function() -> FirFunction {
    let parameter = FirLocalId(0);
    FirFunction {
        owner: DefId(1),
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

fn assert_rejected(function: FirFunction) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed FIR local metadata unexpectedly lowered"),
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
fn native_backends_reject_mismatched_local_ids() {
    let mut function = parameter_function();
    function.locals.get_mut(&FirLocalId(0)).unwrap().id = FirLocalId(7);
    assert_rejected(function);
}

#[test]
fn native_backends_reject_non_parameter_function_parameters() {
    let mut function = parameter_function();
    function
        .locals
        .get_mut(&FirLocalId(0))
        .unwrap()
        .parameter = false;
    assert_rejected(function);
}
