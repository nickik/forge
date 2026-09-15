use cranelift_codegen::isa::CallConv;
use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{ConstValue, DefId, FirGlobal, FirModule, Ty};

#[test]
fn aarch64_backend_initializes() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Aarch64);
    assert_eq!(
        backend.target_triple().to_string(),
        "aarch64-unknown-linux-gnu"
    );
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn riscv64_backend_initializes() {
    let backend = CraneliftBackend::riscv64().expect("RISC-V64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Riscv64);
    assert_eq!(
        backend.target_triple().to_string(),
        "riscv64gc-unknown-linux-gnu"
    );
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn empty_module_prepares_empty_function_set() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let prepared = backend
        .prepare_module(&FirModule::default())
        .expect("empty FIR");
    assert!(prepared.functions().is_empty());
    assert!(prepared.globals().is_empty());
}

#[test]
fn global_is_prepared_but_emission_stops_at_c11b_boundary() {
    let backend = CraneliftBackend::aarch64().expect("backend");
    let mut module = FirModule::default();
    module.globals.insert(
        DefId(1),
        FirGlobal {
            owner: DefId(1),
            ty: Ty::Bool,
            constant: Some(ConstValue::Bool { value: true }),
        },
    );

    let prepared = backend
        .prepare_module(&module)
        .expect("C11a prepares global metadata");
    assert!(prepared.global(DefId(1)).is_some());

    let plan = backend
        .plan_object_module(&prepared)
        .expect("C11a plans global symbols");
    assert!(plan.global_symbol(DefId(1)).is_some());

    let error = backend
        .emit_object(&prepared, &plan)
        .expect_err("C11b must own global section emission");
    assert_eq!(
        error,
        BackendError::UnsupportedFir {
            component: "global object emission"
        }
    );
}

#[test]
fn backend_error_display_is_stable() {
    assert_eq!(
        BackendError::UnsupportedInstruction { kind: "call" }.to_string(),
        "FIR instruction is not lowered to CLIF yet: call"
    );
}
