use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind, FirLocal,
    FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u(width: IntWidth) -> Ty {
    Ty::Int {
        signed: false,
        width,
    }
}

fn bounds_check(index_ty: Ty, len_ty: Ty) -> FirFunction {
    let index_local = FirLocalId(0);
    let len_local = FirLocalId(1);
    let index = FirValueId(0);
    let len = FirValueId(1);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![index_local, len_local],
        return_type: Ty::Void,
        locals: BTreeMap::from([
            (
                index_local,
                FirLocal {
                    id: index_local,
                    source: None,
                    ty: index_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                len_local,
                FirLocal {
                    id: len_local,
                    source: None,
                    ty: len_ty.clone(),
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
                    span,
                    result: Some(index),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: index_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(len),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: len_local },
                    },
                },
                FirInstruction {
                    span,
                    result: None,
                    kind: FirInstructionKind::BoundsCheck { index, len },
                },
            ],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: BTreeMap::from([(index, index_ty), (len, len_ty)]),
    }
}

fn index_unchecked(base_ty: Ty, index_ty: Ty, result_ty: Ty) -> FirFunction {
    let base_local = FirLocalId(0);
    let index_local = FirLocalId(1);
    let base = FirValueId(0);
    let index = FirValueId(1);
    let result = FirValueId(2);
    let span = Span::new(0, 0);
    FirFunction {
        owner: DefId(1),
        params: vec![base_local, index_local],
        return_type: result_ty.clone(),
        locals: BTreeMap::from([
            (
                base_local,
                FirLocal {
                    id: base_local,
                    source: None,
                    ty: base_ty.clone(),
                    mutable: false,
                    parameter: true,
                    synthetic: false,
                },
            ),
            (
                index_local,
                FirLocal {
                    id: index_local,
                    source: None,
                    ty: index_ty.clone(),
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
                    span,
                    result: Some(base),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: base_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(index),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: index_local },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(result),
                    kind: FirInstructionKind::IndexUnchecked { base, index },
                },
            ],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(base, base_ty), (index, index_ty), (result, result_ty)]),
    }
}

fn assert_invalid(function: FirFunction, message: &str) {
    let mut module = FirModule::default();
    module.functions.insert(function.owner, function);
    let definitions = TypeDefinitionTable::new();
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &definitions) {
            Ok(_) => panic!("malformed indexing FIR unexpectedly lowered"),
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
fn bounds_check_requires_a_usize_index() {
    assert_invalid(
        bounds_check(u(IntWidth::W32), u(IntWidth::Pointer)),
        "bounds-check index has non-usize FIR type Int { signed: false, width: W32 }",
    );
}

#[test]
fn bounds_check_requires_a_usize_length() {
    assert_invalid(
        bounds_check(u(IntWidth::Pointer), u(IntWidth::W32)),
        "bounds-check length has non-usize FIR type Int { signed: false, width: W32 }",
    );
}

#[test]
fn unchecked_index_requires_a_sequence_base() {
    assert_invalid(
        index_unchecked(u(IntWidth::W32), u(IntWidth::Pointer), u(IntWidth::W16)),
        "index-unchecked instruction has unsupported FIR base type Int { signed: false, width: W32 }",
    );
}

#[test]
fn unchecked_index_requires_a_usize_index() {
    assert_invalid(
        index_unchecked(
            Ty::Slice {
                mutable: false,
                element: Box::new(u(IntWidth::W16)),
            },
            u(IntWidth::W32),
            u(IntWidth::W16),
        ),
        "index-unchecked instruction has non-usize FIR index type Int { signed: false, width: W32 }",
    );
}

#[test]
fn unchecked_index_requires_the_exact_element_result() {
    assert_invalid(
        index_unchecked(
            Ty::Slice {
                mutable: false,
                element: Box::new(u(IntWidth::W16)),
            },
            u(IntWidth::Pointer),
            u(IntWidth::W32),
        ),
        "index-unchecked instruction has FIR result type Int { signed: false, width: W32 }, indexed element type is Int { signed: false, width: W16 }",
    );
}
