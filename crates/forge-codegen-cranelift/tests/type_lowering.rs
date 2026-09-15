use cranelift_codegen::ir::types;
use forge_codegen_cranelift::{BackendError, CraneliftBackend, TargetLayout, TypeLowering};
use forge_fir::{IntWidth, Ty};

fn int(signed: bool, width: IntWidth) -> Ty {
    Ty::Int { signed, width }
}

fn assert_scalar_mappings(backend: &CraneliftBackend) {
    let lowering = backend.type_lowering();

    let cases = [
        (Ty::Bool, types::I8),
        (Ty::Byte, types::I8),
        (int(false, IntWidth::W8), types::I8),
        (int(true, IntWidth::W8), types::I8),
        (int(false, IntWidth::W16), types::I16),
        (int(true, IntWidth::W16), types::I16),
        (int(false, IntWidth::W32), types::I32),
        (int(true, IntWidth::W32), types::I32),
        (int(false, IntWidth::W64), types::I64),
        (int(true, IntWidth::W64), types::I64),
        (int(false, IntWidth::Pointer), types::I64),
        (int(true, IntWidth::Pointer), types::I64),
    ];

    for (ty, expected) in cases {
        assert_eq!(lowering.value_type(&ty), Ok(expected), "mapping for {ty:?}");
    }

    let raw_pointer = Ty::Pointer {
        volatile: false,
        inner: Box::new(Ty::Bool),
    };
    let volatile_pointer = Ty::Pointer {
        volatile: true,
        inner: Box::new(int(false, IntWidth::W64)),
    };
    let shared_reference = Ty::Reference {
        mutable: false,
        inner: Box::new(Ty::Bool),
    };
    let mutable_reference = Ty::Reference {
        mutable: true,
        inner: Box::new(int(true, IntWidth::W32)),
    };
    let function_pointer = Ty::Function {
        params: vec![int(false, IntWidth::W64)],
        result: Box::new(int(false, IntWidth::W64)),
        named_arguments: false,
    };

    for ty in [
        raw_pointer,
        volatile_pointer,
        shared_reference,
        mutable_reference,
        function_pointer,
    ] {
        assert_eq!(
            lowering.value_type(&ty),
            Ok(types::I64),
            "mapping for {ty:?}"
        );
    }
}

#[test]
fn aarch64_maps_every_c2_scalar_representation() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 backend");
    assert_eq!(backend.target_layout().pointer_bits, 64);
    assert_scalar_mappings(&backend);
}

#[test]
fn riscv64_maps_every_c2_scalar_representation() {
    let backend = CraneliftBackend::riscv64().expect("RISC-V 64 backend");
    assert_eq!(backend.target_layout().pointer_bits, 64);
    assert_scalar_mappings(&backend);
}

#[test]
fn pointer_sized_values_are_target_layout_driven() {
    let layout32 = TargetLayout::new(32);
    let lowering32 = TypeLowering::new(&layout32);

    let pointer_like = [
        int(false, IntWidth::Pointer),
        int(true, IntWidth::Pointer),
        Ty::Pointer {
            volatile: false,
            inner: Box::new(Ty::Bool),
        },
        Ty::Pointer {
            volatile: true,
            inner: Box::new(Ty::Array {
                element: Box::new(Ty::Byte),
                length: Some(8),
            }),
        },
        Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Bool),
        },
        Ty::Reference {
            mutable: true,
            inner: Box::new(Ty::Array {
                element: Box::new(Ty::Byte),
                length: Some(8),
            }),
        },
        Ty::Function {
            params: vec![Ty::Bool, int(false, IntWidth::W64)],
            result: Box::new(Ty::Bool),
            named_arguments: true,
        },
    ];

    for ty in pointer_like {
        assert_eq!(
            lowering32.value_type(&ty),
            Ok(types::I32),
            "32-bit pointer representation for {ty:?}"
        );
    }

    let invalid = TargetLayout::new(24);
    assert_eq!(
        TypeLowering::new(&invalid).value_type(&int(true, IntWidth::Pointer)),
        Err(BackendError::UnsupportedTargetLayout { pointer_bits: 24 })
    );
}

#[test]
fn pointer_representation_does_not_depend_on_pointee_or_signature() {
    let layout = TargetLayout::new(64);
    let lowering = TypeLowering::new(&layout);
    let aggregate = Ty::Array {
        element: Box::new(Ty::Byte),
        length: Some(4),
    };

    for ty in [
        Ty::Pointer {
            volatile: false,
            inner: Box::new(aggregate.clone()),
        },
        Ty::Reference {
            mutable: true,
            inner: Box::new(aggregate.clone()),
        },
        Ty::Function {
            params: vec![aggregate.clone()],
            result: Box::new(aggregate),
            named_arguments: true,
        },
    ] {
        assert_eq!(lowering.value_type(&ty), Ok(types::I64));
    }
}

#[test]
fn aggregates_and_non_c2_values_are_rejected_instead_of_flattened() {
    let layout = TargetLayout::new(64);
    let lowering = TypeLowering::new(&layout);
    let cases = [
        (Ty::Never, "never"),
        (Ty::Void, "void"),
        (Ty::Char, "char"),
        (Ty::Str, "str"),
        (Ty::Duration, "duration"),
        (
            Ty::Array {
                element: Box::new(int(false, IntWidth::W8)),
                length: Some(4),
            },
            "array",
        ),
    ];

    for (ty, kind) in cases {
        assert_eq!(
            lowering.value_type(&ty),
            Err(BackendError::UnsupportedType { kind }),
            "C2 must not invent a representation for {ty:?}"
        );
    }
}

#[test]
fn every_fieldless_frontend_semantic_sentinel_is_a_hard_backend_error() {
    let layout = TargetLayout::new(64);
    let lowering = TypeLowering::new(&layout);
    let cases = [
        (Ty::Error, "error"),
        (Ty::Unknown, "unknown"),
        (Ty::IntLiteral, "integer literal"),
        (Ty::FloatLiteral, "float literal"),
        (Ty::NoneLiteral, "none literal"),
    ];

    for (ty, kind) in cases {
        assert_eq!(
            lowering.value_type(&ty),
            Err(BackendError::SemanticTypeLeak { kind }),
            "semantic sentinel {ty:?} must never reach code generation"
        );
    }
}
