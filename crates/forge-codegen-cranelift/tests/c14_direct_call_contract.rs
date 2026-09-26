use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
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
                    place: FirPlace::Local { local: parameter },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u32_ty())]),
    }
}

fn caller(target: DefId, argument_ty: Ty, result_ty: Ty) -> FirFunction {
    let argument = FirValueId(0);
    let result = FirValueId(1);
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
            instructions: vec![
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(argument),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "1".into() },
                    },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(result),
                    kind: FirInstructionKind::Call {
                        target,
                        args: vec![argument],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(argument, argument_ty), (result, result_ty)]),
    }
}

fn assert_invalid(module: FirModule, message: &str) {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed direct-call FIR unexpectedly lowered"),
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
fn direct_call_requires_a_module_target() {
    let target = DefId(1);
    let function = caller(target, u32_ty(), u32_ty());
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(function.owner, function)]),
            ..FirModule::default()
        },
        "direct call target DefId(1) is not in module",
    );
}

#[test]
fn direct_call_argument_must_match_the_parameter_type() {
    let target = DefId(1);
    let function = caller(target, Ty::Byte, u32_ty());
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(target, callee(target)), (function.owner, function)]),
            ..FirModule::default()
        },
        "direct call argument 0 has type Byte, expected Int { signed: false, width: W32 }",
    );
}

#[test]
fn direct_call_result_must_match_the_callee_type() {
    let target = DefId(1);
    let function = caller(target, u32_ty(), Ty::Byte);
    assert_invalid(
        FirModule {
            functions: BTreeMap::from([(target, callee(target)), (function.owner, function)]),
            ..FirModule::default()
        },
        "direct call result type Byte differs from callee result Int { signed: false, width: W32 }",
    );
}
