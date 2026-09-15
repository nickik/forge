use std::collections::BTreeMap;

use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, GlobalInitialization, GlobalStorageClass,
    ObjectLinkage,
};
use forge_fir::{
    ConstValue, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirGlobal,
    FirGlobalInitializer, FirInstruction, FirInstructionKind, FirModule, FirTerminator, FirValueId,
    IntWidth, Span, Ty, TypeDefinitionTable,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn return_u64(owner: DefId, text: &str) -> FirFunction {
    let value = FirValueId(0);
    FirFunction {
        owner,
        params: vec![],
        return_type: u64_ty(),
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
                    value: FirConst::Integer { text: text.into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, u64_ty())]),
    }
}

fn global(owner: u32, ty: Ty, constant: Option<ConstValue>) -> (DefId, FirGlobal) {
    let owner = DefId(owner);
    (
        owner,
        FirGlobal {
            owner,
            ty,
            constant,
        },
    )
}

fn representative_module() -> FirModule {
    let constant = DefId(10);
    let runtime = DefId(11);
    let zero = DefId(12);
    let aggregate = DefId(13);

    FirModule {
        functions: BTreeMap::new(),
        globals: BTreeMap::from([
            global(
                constant.0,
                u64_ty(),
                Some(ConstValue::Integer { value: 42 }),
            ),
            global(runtime.0, u64_ty(), None),
            global(zero.0, u64_ty(), None),
            global(
                aggregate.0,
                Ty::Array {
                    element: Box::new(u64_ty()),
                    length: Some(5),
                },
                None,
            ),
        ]),
        global_initializers: BTreeMap::from([(
            runtime,
            FirGlobalInitializer {
                owner: runtime,
                dependencies: vec![],
                function: return_u64(runtime, "99"),
            },
        )]),
        global_init_order: vec![runtime],
    }
}

#[test]
fn c11a_prepares_c9_layout_and_initialization_policy_on_both_targets() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_globals(&representative_module(), &TypeDefinitionTable::new())
            .expect("global preparation");

        assert_eq!(prepared.target(), target);
        assert_eq!(
            prepared.globals().keys().copied().collect::<Vec<_>>(),
            vec![DefId(10), DefId(11), DefId(12), DefId(13)]
        );
        assert_eq!(prepared.init_order(), &[DefId(11)]);

        let constant = prepared.global(DefId(10)).expect("constant global");
        assert_eq!(constant.layout().size, 8);
        assert_eq!(constant.layout().align, 8);
        assert_eq!(constant.storage(), GlobalStorageClass::ReadOnlyData);
        assert!(matches!(
            constant.initialization(),
            GlobalInitialization::Constant(ConstValue::Integer { value: 42 })
        ));

        let runtime = prepared.global(DefId(11)).expect("runtime global");
        assert_eq!(runtime.storage(), GlobalStorageClass::ZeroFill);
        assert!(matches!(
            runtime.initialization(),
            GlobalInitialization::Runtime {
                dependencies,
                order: 0
            } if dependencies.is_empty()
        ));

        let zero = prepared.global(DefId(12)).expect("zero global");
        assert_eq!(zero.storage(), GlobalStorageClass::ZeroFill);
        assert_eq!(zero.initialization(), &GlobalInitialization::Zero);

        let aggregate = prepared.global(DefId(13)).expect("aggregate global");
        assert_eq!(aggregate.layout().size, 40);
        assert_eq!(aggregate.layout().align, 8);
    }
}

#[test]
fn c11a_global_symbols_are_deterministic_and_explicitly_exported() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let prepared = backend
        .prepare_globals(&representative_module(), &TypeDefinitionTable::new())
        .expect("global preparation");
    let plan = backend
        .plan_global_objects_with_exports(&prepared, [DefId(10), DefId(13)])
        .expect("global object plan");

    assert_eq!(
        plan.symbols().keys().copied().collect::<Vec<_>>(),
        vec![DefId(10), DefId(11), DefId(12), DefId(13)]
    );
    assert_eq!(
        plan.symbol(DefId(10)).expect("constant symbol").name(),
        "__forge_global_0000000a"
    );
    assert_eq!(
        plan.symbol(DefId(13)).expect("aggregate symbol").name(),
        "__forge_global_0000000d"
    );
    assert_eq!(
        plan.symbol(DefId(10)).expect("exported symbol").linkage(),
        ObjectLinkage::Export
    );
    assert_eq!(
        plan.symbol(DefId(11)).expect("local symbol").linkage(),
        ObjectLinkage::Local
    );
}

#[test]
fn c11a_rejects_compile_time_and_runtime_initializer_conflict() {
    let owner = DefId(20);
    let module = FirModule {
        functions: BTreeMap::new(),
        globals: BTreeMap::from([global(
            owner.0,
            u64_ty(),
            Some(ConstValue::Integer { value: 1 }),
        )]),
        global_initializers: BTreeMap::from([(
            owner,
            FirGlobalInitializer {
                owner,
                dependencies: vec![],
                function: return_u64(owner, "2"),
            },
        )]),
        global_init_order: vec![owner],
    };

    let error = CraneliftBackend::aarch64()
        .expect("backend")
        .prepare_globals(&module, &TypeDefinitionTable::new())
        .expect_err("conflicting initializers must fail");
    assert!(matches!(error, BackendError::InvalidFirShape { .. }));
}

#[test]
fn c11a_rejects_illegal_global_layout() {
    let module = FirModule {
        functions: BTreeMap::new(),
        globals: BTreeMap::from([global(21, Ty::Str, None)]),
        global_initializers: BTreeMap::new(),
        global_init_order: vec![],
    };

    let error = CraneliftBackend::aarch64()
        .expect("backend")
        .prepare_globals(&module, &TypeDefinitionTable::new())
        .expect_err("unsized global must fail layout");
    assert!(matches!(error, BackendError::InvalidFirShape { .. }));
}

#[test]
fn c11a_rejects_cross_category_definition_collision() {
    let owner = DefId(22);
    let module = FirModule {
        functions: BTreeMap::from([(owner, return_u64(owner, "1"))]),
        globals: BTreeMap::from([global(owner.0, u64_ty(), None)]),
        global_initializers: BTreeMap::new(),
        global_init_order: vec![],
    };

    let error = CraneliftBackend::aarch64()
        .expect("backend")
        .prepare_globals(&module, &TypeDefinitionTable::new())
        .expect_err("function/global owner collision must fail");
    assert!(matches!(error, BackendError::InvalidFirShape { .. }));
}

#[test]
fn c11a_preserves_fir_initializer_order_validation() {
    let owner = DefId(23);
    let module = FirModule {
        functions: BTreeMap::new(),
        globals: BTreeMap::from([global(owner.0, u64_ty(), None)]),
        global_initializers: BTreeMap::from([(
            owner,
            FirGlobalInitializer {
                owner,
                dependencies: vec![],
                function: return_u64(owner, "1"),
            },
        )]),
        global_init_order: vec![],
    };

    let error = CraneliftBackend::aarch64()
        .expect("backend")
        .prepare_globals(&module, &TypeDefinitionTable::new())
        .expect_err("missing initializer order entry must fail FIR verification");
    assert!(matches!(error, BackendError::InvalidFir { .. }));
}
