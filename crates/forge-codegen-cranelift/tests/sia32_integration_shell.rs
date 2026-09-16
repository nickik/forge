use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, ExecutableFormat, Sia32IntegrationShell,
    Sia32Object, TargetAbi,
};
use forge_fir::FirModule;

#[test]
fn sia32_configuration_reaches_real_target_contract() {
    let shell = Sia32IntegrationShell;
    assert_eq!(shell.target(), CraneliftTarget::Sia32);
    assert_eq!(shell.triple(), "sia32-unknown-none");
    assert_eq!(shell.pointer_bits(), 32);
    assert_eq!(shell.abi(), TargetAbi::Sia32);
    assert_eq!(shell.executable_format(), ExecutableFormat::Sia32FlatImage);
    shell.validate_target_registration().unwrap();

    let backend = CraneliftBackend::sia32().unwrap();
    assert_eq!(backend.target(), CraneliftTarget::Sia32);
    assert_eq!(backend.target_layout().pointer_bits, 32);
    assert_eq!(backend.target_triple().architecture.to_string(), "sia32");
}

#[test]
fn shell_uses_real_m8_object_writer_and_image_builder() {
    let shell = Sia32IntegrationShell;
    let mut object = Sia32Object::with_sections(
        vec![0x0f, 0xc0, 0xff, 0xcf],
        vec![0x11, 0x22, 0x33, 0x44],
        vec![0xaa, 0xbb, 0xcc, 0xdd],
    );
    object.define_symbol("entry", 0).unwrap();

    let serialized = shell.write_object(&object).unwrap();
    assert!(serialized.starts_with(b"SIAO32\0\x01"));
    let parsed = shell.read_object(&serialized).unwrap();
    assert_eq!(parsed, object);

    let image = shell.build_image(&[parsed], 0x1000, "entry", 8).unwrap();
    assert_eq!(image.entry(), 0x1000);
    assert_eq!(image.layout().load_address, 0x1000);
    assert_eq!(image.layout().text.address, 0x1000);
    assert_eq!(image.layout().rodata.address, 0x1004);
    assert_eq!(image.layout().data.address, 0x1008);
    assert_eq!(image.layout().bss.address, 0x100c);
    assert_eq!(image.bytes().len(), 20);
}

#[test]
fn unfinished_clif_lowering_is_explicitly_rejected() {
    let shell = Sia32IntegrationShell;
    assert_eq!(
        shell.require_clif_lowering().unwrap_err(),
        BackendError::UnfinishedTargetLowering { target: "SIA32" }
    );

    let backend = CraneliftBackend::sia32().unwrap();
    let module = FirModule::default();
    assert_eq!(
        backend.prepare_module(&module).unwrap_err(),
        BackendError::UnfinishedTargetLowering { target: "SIA32" }
    );
}
