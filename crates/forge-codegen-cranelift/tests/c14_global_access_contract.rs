use std::collections::BTreeMap;

use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal, FirInstruction,
    FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty,
    TypeDefinitionTable,
};

fn u32_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W32,
    }
}

fn global(owner: DefId, mutable: bool) -> FirGlobal {
    FirGlobal {
        owner,
        ty: u32_ty(),
        mutable,
        constant: None,
    }
}

fn function(kind: FirInstructionKind, result_ty: Option<Ty>) -> FirFunction {
    let result = result_ty.as_ref().map(|_| FirValueId(0));
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
                result,
                kind,
            }],
            terminator: Some(FirTerminator::Return { value: None }),
        }],
        value_types: result_ty
            .map(|ty| BTreeMap::from([(FirValueId(0), ty)]))
            .unwrap_or_default(),
    }
}

fn assert_invalid(global: FirGlobal, function: FirFunction, message: &str) {
    let module = FirModule {
        functions: BTreeMap::from([(function.owner, function)]),
        globals: BTreeMap::from([(global.owner, global)]),
        ..FirModule::default()
    };
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let error = match backend.prepare_module_with_types(&module, &TypeDefinitionTable::new()) {
            Ok(_) => panic!("malformed global-access FIR unexpectedly lowered"),
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
fn global_load_result_must_match_the_declared_type() {
    let owner = DefId(1);
    assert_invalid(
        global(owner, false),
        function(
            FirInstructionKind::LoadGlobal { global: owner },
            Some(Ty::Byte),
        ),
        "global load result type Byte differs from global type Int { signed: false, width: W32 }",
    );
}

#[test]
fn global_store_value_must_match_the_declared_type() {
    let owner = DefId(1);
    let value = FirValueId(0);
    let mut function = function(
        FirInstructionKind::Const {
            value: FirConst::Integer { text: "1".into() },
        },
        Some(Ty::Byte),
    );
    function.blocks[0].instructions.push(FirInstruction {
        span: Span::new(0, 0),
        result: None,
        kind: FirInstructionKind::StoreGlobal {
            global: owner,
            value,
        },
    });
    assert_invalid(
        global(owner, true),
        function,
        "global store value type Byte differs from global type Int { signed: false, width: W32 }",
    );
}

#[test]
fn global_store_requires_mutable_storage() {
    let owner = DefId(1);
    let value = FirValueId(0);
    let mut function = function(
        FirInstructionKind::Const {
            value: FirConst::Integer { text: "1".into() },
        },
        Some(u32_ty()),
    );
    function.blocks[0].instructions.push(FirInstruction {
        span: Span::new(0, 0),
        result: None,
        kind: FirInstructionKind::StoreGlobal {
            global: owner,
            value,
        },
    });
    assert_invalid(
        global(owner, false),
        function,
        "global store targets immutable global DefId(1)",
    );
}

#[test]
fn global_address_result_must_match_mutability_and_pointee() {
    let owner = DefId(1);
    assert_invalid(
        global(owner, true),
        function(
            FirInstructionKind::AddressOfGlobal {
                global: owner,
                mutable: true,
            },
            Some(Ty::Reference {
                mutable: false,
                inner: Box::new(Ty::Byte),
            }),
        ),
        "global address-of result type Reference { mutable: false, inner: Byte } does not match expected reference type Reference { mutable: true, inner: Int { signed: false, width: W32 } }",
    );
}

#[test]
fn mutable_global_address_requires_mutable_storage() {
    let owner = DefId(1);
    assert_invalid(
        global(owner, false),
        function(
            FirInstructionKind::AddressOfGlobal {
                global: owner,
                mutable: true,
            },
            Some(Ty::Reference {
                mutable: true,
                inner: Box::new(u32_ty()),
            }),
        ),
        "mutable global address targets immutable global DefId(1)",
    );
}
