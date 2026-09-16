use forge_codegen_cranelift::{build_sia32_flat_image, BackendError, Sia32Object, Sia32Section};

#[test]
fn m8_3_builds_deterministic_text_rodata_data_bss_image() {
    let mut main = Sia32Object::with_sections(
        vec![0x0f, 0xc0, 0xff, 0xcf],
        vec![0x11, 0x22, 0x33, 0x44],
        vec![0xaa, 0xbb, 0xcc, 0xdd],
    );
    main.define_symbol("entry", 0).expect("entry");
    main.define_section_symbol("constant", Sia32Section::Rodata, 0)
        .expect("constant");
    main.define_section_symbol("state", Sia32Section::Data, 0)
        .expect("state");

    let image = build_sia32_flat_image(&[main.clone()], 0, "entry", 8).expect("image");
    let layout = image.layout();

    assert_eq!(image.entry(), 0);
    assert_eq!(layout.text.address, 0);
    assert_eq!(layout.text.size, 4);
    assert_eq!(layout.rodata.address, 4);
    assert_eq!(layout.rodata.size, 4);
    assert_eq!(layout.data.address, 8);
    assert_eq!(layout.data.size, 4);
    assert_eq!(layout.bss.address, 12);
    assert_eq!(layout.bss.size, 8);
    assert_eq!(layout.bss.end().expect("bss end"), 20);

    assert_eq!(
        image.bytes(),
        &[
            0x0f, 0xc0, 0xff, 0xcf, // text
            0x11, 0x22, 0x33, 0x44, // rodata
            0xaa, 0xbb, 0xcc, 0xdd, // data
            0, 0, 0, 0, 0, 0, 0, 0, // bss
        ]
    );

    let again = build_sia32_flat_image(&[main], 0, "entry", 8).expect("repeat image");
    assert_eq!(again, image);
}

#[test]
fn m8_3_relocates_against_final_image_addresses() {
    let mut text = Sia32Object::new(vec![0; 4]);
    text.define_symbol("entry", 0).expect("entry");
    text.add_abs32_relocation(0, "external_data", 4)
        .expect("relocation");

    let mut data = Sia32Object::with_sections(Vec::new(), Vec::new(), vec![1, 2, 3, 4, 5, 6, 7, 8]);
    data.define_section_symbol("external_data", Sia32Section::Data, 0)
        .expect("data symbol");

    let image = build_sia32_flat_image(&[text, data], 0x1000, "entry", 0).expect("image");
    assert_eq!(image.entry(), 0x1000);
    assert_eq!(image.layout().data.address, 0x1004);
    assert_eq!(&image.bytes()[0..4], &0x1008u32.to_le_bytes());
}

#[test]
fn m8_3_rejects_invalid_entry_and_32_bit_layout_overflow() {
    let mut data_entry = Sia32Object::with_sections(Vec::new(), Vec::new(), vec![0; 4]);
    data_entry
        .define_section_symbol("entry", Sia32Section::Data, 0)
        .expect("data entry");
    assert!(matches!(
        build_sia32_flat_image(&[data_entry], 0, "entry", 0),
        Err(BackendError::Cranelift { .. })
    ));

    let mut text = Sia32Object::new(vec![0, 0]);
    text.define_symbol("entry", 0).expect("entry");
    assert!(matches!(
        build_sia32_flat_image(&[text], u32::MAX - 1, "entry", 8),
        Err(BackendError::Cranelift { .. })
    ));
}
