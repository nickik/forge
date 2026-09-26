use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn function_ty() -> Ty {
    Ty::Function {
        params: vec![u32_ty()],
        result: Box::new(u32_ty()),
        named_arguments: false,
    }
}

fn caller(callee_ty: Ty, argument_ty: Ty, result_ty: Ty) -> FirFunction {
    let callee_local = FirLocalId(0);
    let argument_local = FirLocalId(1);
    let callee = FirValueId(0);
    let argument = FirValueId(1);
    let result = FirValueId(2);
    FirFunction {
        owner: DefId(1),
        params: vec![callee_local, argument_local],
        return_type: Ty::Void,
        locals: BTreeMap::from([
            (
                callee_local,
                FirLocal {
                    id: callee_local,
                    source: None,
                    ty: callee_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                argument_local,
                FirLocal {
                    id: argument_local,
                    source: None,
                    ty: argument_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
        ]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(callee),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: callee_local,
                        },
                    },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(argument),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: argument_local,
                        },
                    },
                },
                FirInstruction {
                    span: Span::new(0, 0),
                    result: Some(result),
                    kind: FirInstructionKind::CallIndirect {
                        callee,
                        args: vec![argument],
                        tail: false,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([
            (callee, callee_ty),
            (argument, argument_ty),
            (result, result_ty),
        ]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        ..FirModule::default()
    };
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed indirect-call FIR unexpectedly lowered"),
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
fn indirect_call_requires_a_function_typed_callee() {
    assert_invalid(
        caller(Ty::Byte, u32_ty(), u32_ty()),
        "indirect call callee has non-function type Byte",
    );
}

#[test]
fn indirect_call_argument_must_match_the_parameter_type() {
    assert_invalid(
        caller(function_ty(), Ty::Byte, u32_ty()),
        "indirect call argument 0 has type Byte, expected Int { signed: false, width: W32 }",
    );
}

#[test]
fn indirect_call_arity_must_match_the_parameter_list() {
    let callee_ty = Ty::Function {
        params: vec![u32_ty(), u32_ty()],
        result: Box::new(u32_ty()),
        named_arguments: false,
    };
    assert_invalid(
        caller(callee_ty, u32_ty(), u32_ty()),
        "indirect call has 1 argument(s), expected 2",
    );
}

#[test]
fn indirect_call_result_must_match_the_callee_type() {
    assert_invalid(
        caller(function_ty(), u32_ty(), Ty::Byte),
        "indirect call result type Byte differs from callee result Int { signed: false, width: W32 }",
    );
}
