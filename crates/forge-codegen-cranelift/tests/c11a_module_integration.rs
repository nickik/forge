use std::collections::BTreeMap;

use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, GlobalInitialization, ObjectLinkage,
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

fn integrated_module() -> FirModule {
    let function = DefId(1);
    let constant = DefId(10);
    let runtime = DefId(11);
    FirModule {
        functions: BTreeMap::from([(function, return_u64(function, "7"))]),
        globals: BTreeMap::from([
            (
                constant,
                FirGlobal {
                    owner: constant,
                    ty: u64_ty(),
                    constant: Some(ConstValue::Integer { value: 42 }),
                },
            ),
            (
                runtime,
                FirGlobal {
                    owner: runtime,
                    ty: u64_ty(),
                    constant: None,
                },
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
fn c11a_prepared_module_carries_functions_globals_and_init_order() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module_with_types(&integrated_module(), &TypeDefinitionTable::new())
            .expect("integrated prepared module");

        assert_eq!(prepared.target(), target);
        assert_eq!(
            prepared.functions().keys().copied().collect::<Vec<_>>(),
            vec![DefId(1)]
        );
        assert_eq!(
            prepared.globals().keys().copied().collect::<Vec<_>>(),
            vec![DefId(10), DefId(11)]
        );
        assert_eq!(prepared.global_init_order(), &[DefId(11)]);
        assert!(matches!(
            prepared
                .global(DefId(10))
                .expect("constant global")
                .initialization(),
            GlobalInitialization::Constant(ConstValue::Integer { value: 42 })
        ));
        assert!(matches!(
            prepared
                .global(DefId(11))
                .expect("runtime global")
                .initialization(),
            GlobalInitialization::Runtime { order: 0, .. }
        ));
    }
}

#[test]
fn c11a_object_plan_carries_function_and_global_symbols_together() {
    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend
            .prepare_module(&integrated_module())
            .expect("integrated prepared module");
        let plan = backend
            .plan_object_module_with_exports(&prepared, [DefId(1), DefId(10)])
            .expect("integrated object plan");

        assert_eq!(plan.function_symbols().len(), 1);
        assert_eq!(plan.global_symbols().len(), 2);
        assert_eq!(plan.global_init_order(), &[DefId(11)]);
        assert_eq!(
            plan.symbol(DefId(1)).expect("function symbol").name(),
            "__forge_fn_00000001"
        );
        assert_eq!(
            plan.global_symbol(DefId(10)).expect("global symbol").name(),
            "__forge_global_0000000a"
        );
        assert_eq!(
            plan.symbol(DefId(1)).expect("function symbol").linkage(),
            ObjectLinkage::Export
        );
        assert_eq!(
            plan.global_symbol(DefId(10))
                .expect("global symbol")
                .linkage(),
            ObjectLinkage::Export
        );
        assert_eq!(
            plan.global_symbol(DefId(11))
                .expect("runtime symbol")
                .linkage(),
            ObjectLinkage::Local
        );
    }
}

#[test]
fn c11a_object_emission_refuses_to_silently_drop_planned_globals() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let prepared = backend
        .prepare_module(&integrated_module())
        .expect("prepared");
    let plan = backend.plan_object_module(&prepared).expect("plan");
    let error = backend
        .emit_object(&prepared, &plan)
        .expect_err("C11b must own global section emission");
    assert!(matches!(
        error,
        BackendError::UnsupportedFir {
            component: "global object emission"
        }
    ));
}

#[test]
fn c11a_function_only_c10_object_path_remains_unchanged() {
    let owner = DefId(2);
    let module = FirModule {
        functions: BTreeMap::from([(owner, return_u64(owner, "5"))]),
        ..FirModule::default()
    };

    for target in [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64] {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend.prepare_module(&module).expect("prepared");
        assert!(prepared.globals().is_empty());
        assert!(prepared.global_init_order().is_empty());

        let plan = backend
            .plan_object_module_with_exports(&prepared, [owner])
            .expect("object plan");
        assert!(plan.global_symbols().is_empty());
        assert!(plan.global_init_order().is_empty());

        let object = backend.emit_object(&prepared, &plan).expect("C10 emission");
        assert_eq!(&object.bytes()[..4], b"\x7fELF");
    }
}
