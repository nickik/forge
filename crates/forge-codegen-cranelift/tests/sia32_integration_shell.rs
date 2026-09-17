use std::collections::BTreeMap;

use cranelift_codegen::ir::types;
use forge_codegen_cranelift::{
    BackendError, CraneliftBackend, CraneliftTarget, ExecutableFormat, Sia32IntegrationShell,
    Sia32Object, TargetAbi,
};
use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction, FirInstructionKind,
    FirModule, FirTerminator, FirValueId, IntWidth, Span, Ty,
};

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
fn sia32_pointer_sized_values_are_32_bit_before_general_lowering() {
    let backend = CraneliftBackend::sia32().unwrap();
    let lowering = backend.type_lowering();
    let usize_ty = Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    };

    assert_eq!(lowering.pointer_type().unwrap(), types::I32);
    assert_eq!(lowering.value_type(&usize_ty).unwrap(), types::I32);
    assert_eq!(lowering.scalar_layout(&usize_ty).unwrap().size_bytes, 4);
    assert_eq!(lowering.scalar_layout(&usize_ty).unwrap().align_bytes, 4);
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
fn sia32_compiles_a_small_forge_function_to_native_bytes() {
    let shell = Sia32IntegrationShell;
    shell.require_clif_lowering().unwrap();

    let backend = CraneliftBackend::sia32().unwrap();
    let owner = DefId(0);
    let value = FirValueId(0);
    let integer = Ty::Int {
        signed: true,
        width: IntWidth::W32,
    };
    let function = FirFunction {
        owner,
        params: vec![],
        return_type: integer.clone(),
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
                    value: FirConst::Integer { text: "42".into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, integer)]),
    };
    let mut module = FirModule::default();
    module.functions.insert(owner, function);

    let prepared = backend.prepare_module(&module).unwrap();
    let code = backend.emit_machine_code(&prepared, owner).unwrap();
    assert_eq!(code.target(), CraneliftTarget::Sia32);
    assert!(!code.bytes().is_empty());
    assert_eq!(code.bytes().len() % 2, 0);
    assert_eq!(&code.bytes()[code.bytes().len() - 2..], [0xe0, 0xc0]);
}

#[test]
fn sia32_rejects_float_fir_before_isa_lowering() {
    let owner = DefId(0);
    let value = FirValueId(0);
    let float = Ty::Float { bits: 32 };
    let function = FirFunction {
        owner,
        params: vec![],
        return_type: float.clone(),
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
                    value: FirConst::Float { text: "1.0f32".into() },
                },
            }],
            terminator: Some(FirTerminator::Return { value: Some(value) }),
        }],
        value_types: BTreeMap::from([(value, float)]),
    };
    let mut module = FirModule::default();
    module.functions.insert(owner, function);

    match CraneliftBackend::sia32().unwrap().prepare_module(&module) {
        Err(BackendError::UnsupportedFir {
            component: "floating point on SIA32 (deferred)",
        }) => {}
        Err(error) => panic!("wrong SIA32 float rejection: {error}"),
        Ok(_) => panic!("SIA32 unexpectedly accepted float FIR"),
    }
}
