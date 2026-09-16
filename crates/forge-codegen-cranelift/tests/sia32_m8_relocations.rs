use cranelift_codegen::binemit::Reloc;
use forge_codegen_cranelift::{link_sia32_objects, BackendError, Sia32Object};

#[test]
fn m8_records_exact_abs32_call_relocation_contract() {
    // Canonical backend long call:
    //   LDPC.W r12, literal
    //   CALLR r12
    //   B after_literal
    //   padding
    //   .word callee       <- ABS32 at byte 8
    let mut caller = Sia32Object::new(vec![0; 12]);
    caller
        .add_abs32_relocation(8, "__forge_fn_00000002", 0)
        .expect("call relocation");

    let relocation = &caller.relocations()[0];
    assert_eq!(relocation.offset(), 8);
    assert_eq!(relocation.kind(), Reloc::Abs4);
    assert_eq!(relocation.symbol(), "__forge_fn_00000002");
    assert_eq!(relocation.addend(), 0);
    assert_eq!(&caller.bytes()[8..12], &[0, 0, 0, 0]);
}

#[test]
fn m8_links_cross_object_call_and_data_references() {
    let mut caller = Sia32Object::new(vec![0; 16]);
    caller
        .define_symbol("__forge_fn_00000001", 0)
        .expect("caller symbol");
    caller
        .add_abs32_relocation(8, "__forge_fn_00000002", 0)
        .expect("callee relocation");
    caller
        .add_abs32_relocation(12, "__forge_global_00000003", -4)
        .expect("data relocation");

    let mut callee = Sia32Object::new(vec![0; 4]);
    callee
        .define_symbol("__forge_fn_00000002", 0)
        .expect("callee symbol");

    let mut data = Sia32Object::new(vec![0; 16]);
    data.define_symbol("__forge_global_00000003", 8)
        .expect("data symbol");

    let linked = link_sia32_objects(
        &[caller, callee, data],
        &[0x0000_1000, 0x0000_2000, 0x0000_3000],
    )
    .expect("link SIA32 objects");

    assert_eq!(&linked[0][8..12], &0x0000_2000u32.to_le_bytes());
    // S = 0x3008, A = -4.
    assert_eq!(&linked[0][12..16], &0x0000_3004u32.to_le_bytes());
}

#[test]
fn m8_rejects_unresolved_unsupported_unaligned_and_overflowing_relocations() {
    let mut unresolved = Sia32Object::new(vec![0; 4]);
    unresolved
        .add_abs32_relocation(0, "missing", 0)
        .expect("record unresolved relocation");
    assert!(matches!(
        link_sia32_objects(&[unresolved], &[0]),
        Err(BackendError::Cranelift { .. })
    ));

    let mut unsupported = Sia32Object::new(vec![0; 8]);
    assert!(matches!(
        unsupported.add_relocation(0, Reloc::Abs8, "symbol", 0),
        Err(BackendError::Cranelift { .. })
    ));

    let mut unaligned = Sia32Object::new(vec![0; 8]);
    assert!(matches!(
        unaligned.add_abs32_relocation(2, "symbol", 0),
        Err(BackendError::Cranelift { .. })
    ));

    let mut underflow = Sia32Object::new(vec![0; 4]);
    underflow.define_symbol("zero", 0).expect("zero symbol");
    underflow
        .add_abs32_relocation(0, "zero", -1)
        .expect("underflow relocation");
    assert!(matches!(
        link_sia32_objects(&[underflow], &[0]),
        Err(BackendError::Cranelift { .. })
    ));

    let mut overflow = Sia32Object::new(vec![0; 4]);
    overflow.define_symbol("top", 0).expect("top symbol");
    overflow
        .add_abs32_relocation(0, "top", 1)
        .expect("overflow relocation");
    assert!(matches!(
        link_sia32_objects(&[overflow], &[u32::MAX]),
        Err(BackendError::Cranelift { .. })
    ));
}
