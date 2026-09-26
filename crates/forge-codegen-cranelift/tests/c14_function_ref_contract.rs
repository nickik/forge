use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty, TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn function_ty(params: Vec<Ty>, result: Ty) -> Ty {
    Ty::Function {
        params,
        result: Box::new(result),
        named_arguments: false,
    }
}

fn callee(owner: DefId) -> FirFunction {
    let parameter = FirLocalId(0);
    let value = FirValueId(0);
    FirFunction {
        owner,
        params: vec![parameter],
        return_type: u32_ty(),
        locals: BTreeMap::from([(
            parameter,
            FirLocal {
                id: parameter,
                source: None,
                ty: u32_ty(),
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
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(value),
                kind: FirInstructionKind::Load {
                    place: forge_fir::FirPlace::Local { local: parameter },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u32_ty())]),
    }
}

fn caller(target: DefId, result_ty: Ty) -> FirFunction {
    let result = FirValueId(0);
    FirFunction {
        owner: DefId(2),
        params: vec![],
        return_type: Ty::Void,
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(result),
                kind: FirInstructionKind::FunctionRef { target },
            }],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(result, result_ty)]),
    }
}

fn assert_invalid(module: FirModule, message: &str) {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed function-reference FIR unexpectedly lowered"),
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
fn function_reference_requires_a_module_target() {
    let target = DefId(1);
    let function = caller(target, function_ty(vec![u32_ty()], u32_ty()));
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(function.owner, function)]),
            ..FirModule::default()
        },
        "function-ref target DefId(1) is not in module",
    );
}

#[test]
fn function_reference_requires_a_function_result_type() {
    let target = DefId(1);
    let function = caller(target, Ty::Byte);
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(target, callee(target)), (function.owner, function)]),
            ..FirModule::default()
        },
        "function-ref has non-function result type Byte",
    );
}

#[test]
fn function_reference_must_match_the_target_signature() {
    let target = DefId(1);
    let function = caller(target, function_ty(vec![Ty::Byte], u32_ty()));
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(target, callee(target)), (function.owner, function)]),
            ..FirModule::default()
        },
        "function-ref type Function { params: [Byte], result: Int { signed: false, width: W32 }, named_arguments: false } does not match target DefId(1) signature",
    );
}
