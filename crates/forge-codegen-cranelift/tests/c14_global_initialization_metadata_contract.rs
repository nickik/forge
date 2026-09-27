use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal,
    FirGlobalInitializer, FirInstruction, FirInstructionKind, FirModule, FirTerminator, FirValueId,
    IntWidth, Span, Ty, TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn return_u32(owner: DefId) -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner,
        params: Vec::new(),
        return_type: u32_ty(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(value),
                kind: FirInstructionKind::Const {
                    value: FirConst::Integer { text: "1".into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u32_ty())]),
    }
}

fn mismatched_constant_module() -> FirModule {
    let owner = DefId(1);
    FirModule {
        globals: BTreeMap::from([(
            owner,
            FirGlobal {
                owner,
                ty: Ty::Bool,
                mutable: false,
                constant: Some(ConstValue::Integer { value: 1 }),
            },
        )]),
        ..FirModule::default()
    }
}

fn conflicting_initializer_module() -> FirModule {
    let owner = DefId(2);
    FirModule {
        globals: BTreeMap::from([(
            owner,
            FirGlobal {
                owner,
                ty: u32_ty(),
                mutable: false,
                constant: Some(ConstValue::Integer { value: 1 }),
            },
        )]),
        global_initializers: BTreeMap::from([(
            owner,
            FirGlobalInitializer {
                owner,
                dependencies: Vec::new(),
                function: return_u32(owner),
            },
        )]),
        global_init_order: vec![owner],
        ..FirModule::default()
    }
}

#[test]
fn native_backends_reject_invalid_global_initialization_metadata() {
    for module in [
        mismatched_constant_module(),
        conflicting_initializer_module(),
    ] {
        for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
            let backend = CraneliftBackend::new(target).expect("backend");
            let error =
                match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
                    Ok(_) => panic!("invalid global initialization metadata unexpectedly lowered"),
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
}
