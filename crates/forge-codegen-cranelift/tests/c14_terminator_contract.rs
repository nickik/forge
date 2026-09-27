use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    verify_fir_module, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn assert_invalid(function: FirFunction, code: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let diagnostics = verify_fir_module(&module);
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, code);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed terminator FIR unexpectedly lowered"),
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

fn constant_function(return_type: Ty, value_type: Ty, terminator: FirTerminator) -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type,
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
                    value: FirConst::Bool { value: true },
                },
            }],
            terminator: Some(terminator),
        }],
        value_types: BTreeMap::from([(value, value_type)]),
    }
}

#[test]
fn branch_condition_requires_bool() {
    let condition = FirValueId(0);
    let mut function = constant_function(
        Ty::Void,
        u32_ty(),
        FirTerminator::Branch {
            condition,
            then_block: FirBlockId(0),
            else_block: FirBlockId(0),
        },
    );
    function.blocks[0].instructions[0].kind = FirInstructionKind::Const {
        value: FirConst::Integer {
            text: "1u32".into(),
        },
    };
    assert_invalid(function, "fir/verify-branch");
}

#[test]
fn return_value_requires_the_callable_result_type() {
    let value = FirValueId(0);
    assert_invalid(
        constant_function(
            u32_ty(),
            Ty::Bool,
            FirTerminator::Return { value: Some(value) },
        ),
        "fir/verify-return",
    );
}

#[test]
fn non_void_return_requires_a_value() {
    let function = FirFunction {
        owner: DefId(1),
        params: Vec::new(),
        return_type: u32_ty(),
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
    };
    assert_invalid(
        function,
        "fir/verify-return",
    );
}
