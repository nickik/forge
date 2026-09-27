use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, ExprId, FirBasicBlock, FirBlockId, FirClosure, FirFunction, FirModule, FirTerminator,
    Ty, TypeDefinitionTable,
};

fn single_block_function() -> FirFunction {
    FirFunction {
        owner: DefId(1),
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

fn assert_rejected(function: FirFunction) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed FIR block identity unexpectedly lowered"),
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
fn native_backends_reject_mismatched_block_ids() {
    let mut function = single_block_function();
    function.blocks[0].id = FirBlockId(7);
    assert_rejected(function);
}

#[test]
fn native_backends_reject_closure_owned_function_entries() {
    let mut function = single_block_function();
    let closure = ExprId(7);
    function.closures.insert(
        closure,
        FirClosure {
            id: closure,
            captures: Vec::new(),
            params: Vec::new(),
            return_type: Ty::Void,
            entry: FirBlockId(0),
            function_pointer: false,
        },
    );
    function.blocks[0].closure = Some(closure);
    assert_rejected(function);
}
