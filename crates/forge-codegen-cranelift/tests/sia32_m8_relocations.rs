use cranelift_codegen::binemit::Reloc;
use forge_codegen_cranelift::{
    link_sia32_objects, link_sia32_sectioned_objects, BackendError, Sia32Object, Sia32Section,
    Sia32SectionBases,
};

#[test]
fn m8_records_exact_abs32_call_relocation_contract() {
    let mut caller = Sia32Object::new(vec![0; 12]);
    caller.add_abs32_relocation(8, "__forge_fn_00000002", 0).unwrap();
    let relocation = &caller.relocations()[0];
    assert_eq!(relocation.section(), Sia32Section::Text);
    assert_eq!(relocation.offset(), 8);
    assert_eq!(relocation.kind(), Reloc::Abs4);
    assert_eq!(relocation.symbol(), "__forge_fn_00000002");
    assert_eq!(relocation.addend(), 0);
}

#[test]
fn m8_real_object_roundtrip_preserves_sections_symbols_relocations() {
    let mut object = Sia32Object::with_sections(vec![0xaa; 12], vec![1, 2, 3, 4], vec![0; 8]);
    object.define_section_symbol("entry", Sia32Section::Text, 0).unwrap();
    object.define_section_symbol("constant", Sia32Section::Rodata, 0).unwrap();
    object.define_section_symbol("global", Sia32Section::Data, 4).unwrap();
    object.add_section_abs32_relocation(Sia32Section::Text, 8, "global", -4).unwrap();
    object.add_section_abs32_relocation(Sia32Section::Data, 0, "constant", 0).unwrap();

    let encoded = object.to_bytes().expect("emit SIAO32");
    assert_eq!(&encoded[..8], b"SIAO32\0\x01");
    let parsed = Sia32Object::from_bytes(&encoded).expect("parse SIAO32");
    assert_eq!(parsed, object);
    assert_eq!(parsed.section(Sia32Section::Text), &[0xaa; 12]);
    assert_eq!(parsed.section(Sia32Section::Rodata), &[1, 2, 3, 4]);
    assert_eq!(parsed.section(Sia32Section::Data), &[0; 8]);
    assert_eq!(parsed.symbols()["global"].section(), Sia32Section::Data);
    assert_eq!(parsed.symbols()["global"].offset(), 4);
    assert_eq!(parsed.relocations()[0].offset(), 8);
    assert_eq!(parsed.relocations()[0].addend(), -4);
}

#[test]
fn m8_links_three_real_objects_call_and_data_end_to_end() {
    // main.o: canonical long-call literal at text+8 and pointer to data at text+12.
    let mut main = Sia32Object::with_sections(vec![0; 16], vec![], vec![]);
    main.define_symbol("main", 0).unwrap();
    main.add_abs32_relocation(8, "worker", 0).unwrap();
    main.add_abs32_relocation(12, "counter", 4).unwrap();

    let mut function = Sia32Object::with_sections(vec![0xcc; 4], vec![0; 4], vec![]);
    function.define_symbol("worker", 0).unwrap();
    function.define_section_symbol("answer", Sia32Section::Rodata, 0).unwrap();

    let mut data = Sia32Object::with_sections(vec![], vec![], vec![0; 8]);
    data.define_section_symbol("counter", Sia32Section::Data, 0).unwrap();
    data.add_section_abs32_relocation(Sia32Section::Data, 0, "answer", 0).unwrap();

    // Exercise the actual object boundary: serialize, then parse before linking.
    let objects = [&main, &function, &data]
        .into_iter()
        .map(|o| Sia32Object::from_bytes(&o.to_bytes().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let bases = [
        Sia32SectionBases { text: 0x1000, rodata: 0x1800, data: 0x1c00 },
        Sia32SectionBases { text: 0x2000, rodata: 0x2400, data: 0x2800 },
        Sia32SectionBases { text: 0x3000, rodata: 0x3400, data: 0x3800 },
    ];
    let linked = link_sia32_sectioned_objects(&objects, &bases).expect("link main/function/data");
    assert_eq!(&linked[0].text[8..12], &0x2000u32.to_le_bytes());
    assert_eq!(&linked[0].text[12..16], &0x3804u32.to_le_bytes());
    assert_eq!(&linked[2].data[0..4], &0x2400u32.to_le_bytes());
}

#[test]
fn m8_links_legacy_flat_objects() {
    let mut caller = Sia32Object::new(vec![0; 16]);
    caller.define_symbol("caller", 0).unwrap();
    caller.add_abs32_relocation(8, "callee", 0).unwrap();
    caller.add_abs32_relocation(12, "data", -4).unwrap();
    let mut callee = Sia32Object::new(vec![0; 4]);
    callee.define_symbol("callee", 0).unwrap();
    let mut data = Sia32Object::new(vec![0; 16]);
    data.define_symbol("data", 8).unwrap();
    let linked = link_sia32_objects(&[caller, callee, data], &[0x1000, 0x2000, 0x3000]).unwrap();
    assert_eq!(&linked[0][8..12], &0x2000u32.to_le_bytes());
    assert_eq!(&linked[0][12..16], &0x3004u32.to_le_bytes());
}

#[test]
fn m8_rejects_unresolved_duplicates_malformed_and_overflowing_relocations() {
    let mut unresolved = Sia32Object::new(vec![0; 4]);
    unresolved.add_abs32_relocation(0, "missing", 0).unwrap();
    assert!(matches!(link_sia32_objects(&[unresolved], &[0]), Err(BackendError::Cranelift { .. })));

    let mut unsupported = Sia32Object::new(vec![0; 8]);
    assert!(matches!(unsupported.add_relocation(0, Reloc::Abs8, "symbol", 0), Err(BackendError::Cranelift { .. })));

    let mut unaligned = Sia32Object::new(vec![0; 8]);
    assert!(matches!(unaligned.add_abs32_relocation(2, "symbol", 0), Err(BackendError::Cranelift { .. })));

    let mut underflow = Sia32Object::new(vec![0; 4]);
    underflow.define_symbol("zero", 0).unwrap();
    underflow.add_abs32_relocation(0, "zero", -1).unwrap();
    assert!(matches!(link_sia32_objects(&[underflow], &[0]), Err(BackendError::Cranelift { .. })));

    let mut overflow = Sia32Object::new(vec![0; 4]);
    overflow.define_symbol("top", 0).unwrap();
    overflow.add_abs32_relocation(0, "top", 1).unwrap();
    assert!(matches!(link_sia32_objects(&[overflow], &[u32::MAX]), Err(BackendError::Cranelift { .. })));

    let mut a = Sia32Object::new(vec![0; 4]); a.define_symbol("dup", 0).unwrap();
    let mut b = Sia32Object::new(vec![0; 4]); b.define_symbol("dup", 0).unwrap();
    assert!(matches!(link_sia32_objects(&[a, b], &[0x1000, 0x2000]), Err(BackendError::Cranelift { .. })));

    let mut malformed = Sia32Object::new(vec![0; 4]).to_bytes().unwrap();
    malformed.push(0);
    assert!(matches!(Sia32Object::from_bytes(&malformed), Err(BackendError::Cranelift { .. })));
}
