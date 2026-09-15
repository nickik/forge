use std::collections::BTreeMap;

use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, ObjectLinkage,
};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn constant_function(owner: DefId, text: &str) -> FirFunction {
    let result = FirValueId(0);
    let ty = u64_ty();
    FirFunction {
        owner,
        params: vec![],
        return_type: ty.clone(),
        locals: BTreeMap::new(),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![FirInstruction {
                span: Span::new(0, 0),
                result: Some(result),
                kind: FirInstructionKind::Const {
                    value: FirConst::Integer { text: text.into() },
                },
            }],
            terminator: Some(FirTerminator::Return {
                value: Some(result),
            }),
        }],
        value_types: BTreeMap::from([(result, ty)]),
    }
}

fn module() -> FirModule {
    let mut module = FirModule::default();
    let later = DefId(9);
    let earlier = DefId(2);
    // Deliberately insert in reverse semantic order. FirModule uses a BTreeMap,
    // and the object plan must remain independent of construction order.
    module.functions.insert(later, constant_function(later, "9"));
    module
        .functions
        .insert(earlier, constant_function(earlier, "2"));
    module
}

fn targets() -> [CraneliftTarget; 2] {
    [CraneliftTarget::Aarch64, CraneliftTarget::Riscv64]
}

#[test]
fn c10a_symbols_are_deterministic_and_preserve_c9_signatures() {
    for target in targets() {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend.prepare_module(&module()).expect("prepare module");
        let plan = backend.plan_object_module(&prepared).expect("object plan");

        assert_eq!(plan.target(), target);
        let symbols: Vec<_> = plan.symbols().values().collect();
        assert_eq!(symbols.len(), 2);
        assert_eq!(symbols[0].owner(), DefId(2));
        assert_eq!(symbols[0].name(), "__forge_fn_00000002");
        assert_eq!(symbols[1].owner(), DefId(9));
        assert_eq!(symbols[1].name(), "__forge_fn_00000009");
        assert_eq!(symbols[0].linkage(), ObjectLinkage::Local);
        assert_eq!(symbols[1].linkage(), ObjectLinkage::Local);

        for symbol in symbols {
            let prepared_function = prepared.function(symbol.owner()).expect("prepared function");
            assert_eq!(
                format!("{:?}", symbol.signature()),
                format!("{:?}", prepared_function.signature),
                "C10a must carry the exact C9-lowered signature into the object plan"
            );
        }
    }
}

#[test]
fn c10a_exports_are_explicit_and_missing_exports_are_rejected() {
    for target in targets() {
        let backend = CraneliftBackend::new(target).expect("backend");
        let prepared = backend.prepare_module(&module()).expect("prepare module");
        let plan = backend
            .plan_object_module_with_exports(&prepared, [DefId(9)])
            .expect("object plan with export");

        assert_eq!(
            plan.symbol(DefId(2)).expect("local").linkage(),
            ObjectLinkage::Local
        );
        assert_eq!(
            plan.symbol(DefId(9)).expect("export").linkage(),
            ObjectLinkage::Export
        );

        let error = backend
            .plan_object_module_with_exports(&prepared, [DefId(99)])
            .expect_err("missing export must be rejected");
        assert!(matches!(error, BackendError::InvalidFirShape { .. }));
    }
}

#[test]
fn c10a_rejects_prepared_modules_for_a_different_target() {
    let aarch64 = CraneliftBackend::aarch64().expect("aarch64 backend");
    let riscv64 = CraneliftBackend::riscv64().expect("riscv64 backend");
    let prepared = aarch64.prepare_module(&module()).expect("prepare module");

    let error = riscv64
        .plan_object_module(&prepared)
        .expect_err("target mismatch must be rejected");
    assert!(matches!(error, BackendError::InvalidFirShape { .. }));
}
