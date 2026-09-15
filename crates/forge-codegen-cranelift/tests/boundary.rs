use cranelift_codegen::isa::CallConv;
use forge_codegen_cranelift::{BackendError, CraneliftBackend, CraneliftTarget};
use forge_fir::{ConstValue, DefId, FirGlobal, FirModule, Ty};

#[test]
fn aarch64_backend_initializes() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Aarch64);
    assert_eq!(backend.target_triple().to_string(), "aarch64-unknown-linux-gnu");
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn riscv64_backend_initializes() {
    let backend = CraneliftBackend::riscv64().expect("RISC-V64 Cranelift backend");
    assert_eq!(backend.target(), CraneliftTarget::Riscv64);
    assert_eq!(backend.target_triple().to_string(), "riscv64gc-unknown-linux-gnu");
    assert_eq!(backend.new_signature().call_conv, CallConv::SystemV);
}

#[test]
fn verified_empty_fir_prepares_cranelift_state() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 Cranelift backend");
    let prepared = backend
        .prepare_module(&FirModule::default())
        .expect("empty verified FIR must be accepted");

    assert_eq!(prepared.target(), CraneliftTarget::Aarch64);
    assert!(prepared.functions().is_empty());
}

#[test]
fn verified_but_unsupported_global_fir_is_rejected_explicitly() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 Cranelift backend");
    let owner = DefId(1);
    let mut module = FirModule::default();
    module.globals.insert(
        owner,
        FirGlobal {
            owner,
            ty: Ty::Bool,
            constant: Some(ConstValue::Bool { value: true }),
        },
    );

    let error = match backend.prepare_module(&module) {
        Ok(_) => panic!("C3 must reject globals instead of guessing a lowering"),
        Err(error) => error,
    };
    assert_eq!(
        error,
        BackendError::UnsupportedFir {
            component: "globals"
        }
    );
}

#[test]
fn codegen_boundary_is_package_enforced() {
    let manifest = include_str!("../Cargo.toml");
    assert!(manifest.contains("forge-fir ="));
    assert!(!manifest.contains("forge-frontend"));

    let sources = [
        include_str!("../src/lib.rs"),
        include_str!("../src/backend.rs"),
        include_str!("../src/diagnostic.rs"),
        include_str!("../src/function.rs"),
        include_str!("../src/target.rs"),
        include_str!("../src/types.rs"),
    ]
    .join("\n");
    for forbidden in [
        "BodyHirOutput",
        "TypeCheckOutput",
        "HirExpr",
        "TypedBody",
        "TypedExpr",
        "PatternKind",
        "forge_frontend",
    ] {
        assert!(
            !sources.contains(forbidden),
            "FIR -> CLIF codegen must not depend on semantic frontend surface {forbidden}"
        );
    }
}
