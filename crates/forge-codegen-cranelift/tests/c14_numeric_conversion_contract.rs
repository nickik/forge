use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

fn function(source_ty: Ty, result_ty: Ty, instruction: FirInstructionKind) -> FirFunction {
    let local = FirLocalId(0);
    let source = FirValueId(0);
    let result = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([(
            local,
            FirLocal {
                id: local,
                source: None,
                ty: source_ty.clone(),
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
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(source),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: instruction,
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(source, source_ty), (result, result_ty)]),
    }
}

fn assert_invalid_fir(function: FirFunction) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module(&module) {
            Ok(_) => panic!("malformed numeric conversion unexpectedly verified"),
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
fn integer_to_float_requires_an_integer_source() {
    let target = Ty::Float { bits: 64 };
    assert_invalid_fir(function(
        Ty::Bool,
        target.clone(),
        FirInstructionKind::IntegerToFloat {
            value: FirValueId(0),
            target,
        },
    ));
}

#[test]
fn float_conversion_requires_a_float_source() {
    let target = Ty::Float { bits: 64 };
    assert_invalid_fir(function(
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        },
        target.clone(),
        FirInstructionKind::FloatConvert {
            value: FirValueId(0),
            target,
        },
    ));
}
