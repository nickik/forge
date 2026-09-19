use forge_codegen_cranelift::{build_sia32_flat_image, Sia32Object};

#[test]
fn m8_preserves_word_phase_for_ldpc_call_literals_across_functions() {
    // A 2-byte function before a direct-call function used to place the caller
    // at 0x...2. LDPC.W is relative to align_down(PC+4,4), so moving a function
    // from compile-time phase 0 mod 4 to link-time phase 2 mod 4 retargets the
    // literal by two bytes. Every compiled function must therefore start 4-byte aligned.
    let mut first = Sia32Object::new(vec![0xff, 0xcf]);
    first.define_symbol("entry", 0).unwrap();

    let mut caller = Sia32Object::new(vec![0; 12]);
    caller.define_symbol("caller", 0).unwrap();
    caller.add_abs32_relocation(8, "callee", 0).unwrap();

    let mut callee = Sia32Object::new(vec![0xff, 0xcf]);
    callee.define_symbol("callee", 0).unwrap();

    let image = build_sia32_flat_image(&[first, caller, callee], 0x1000, "entry", 0).unwrap();
    let bytes = image.bytes();

    // first @ 0x1000 (2 bytes), padding -> caller @ 0x1004, literal @ 0x100c,
    // callee @ 0x1010.
    assert_eq!(&bytes[2..4], &[0, 0]);
    assert_eq!(
        u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        0x1010
    );
}

#[test]
fn m8_rejects_halfword_only_load_alignment() {
    let mut object = Sia32Object::new(vec![0xff, 0xcf]);
    object.define_symbol("entry", 0).unwrap();
    assert!(build_sia32_flat_image(&[object], 0x1002, "entry", 0).is_err());
}
