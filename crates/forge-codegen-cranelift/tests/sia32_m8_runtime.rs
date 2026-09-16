use forge_codegen_cranelift::{build_sia32_flat_image, Sia32Object, Sia32Section};
use lighting_simulation::isa::{addi, callr, ldpc_w, lw, ret, sw, trap, NOP};
use lighting_simulation::{Cpu, Stop, DEFAULT_MAX_STEPS};

fn words(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[test]
fn m8_4_links_loads_and_executes_cross_object_call_and_data_access() {
    // Object 1: canonical external-call literal. LDPC.W at PC 0 uses aligned
    // PC+4 as its base, so the literal at byte 8 is displacement +1 word.
    let mut caller = Sia32Object::new(words(&[ldpc_w(12, 1), callr(12), trap(0), NOP, 0, 0]));
    caller.define_symbol("entry", 0).expect("entry");
    caller
        .add_abs32_relocation(8, "callee", 0)
        .expect("external call relocation");

    // Object 2: load the relocated address of external data, increment it,
    // store it back, and return to the caller. Its literal is at text+12.
    let mut callee = Sia32Object::new(words(&[
        ldpc_w(2, 2),
        lw(1, 2),
        addi(1, 1),
        sw(1, 2),
        ret(),
        NOP,
        0,
        0,
    ]));
    callee.define_symbol("callee", 0).expect("callee");
    callee
        .add_abs32_relocation(12, "counter", 0)
        .expect("external data relocation");

    let mut data = Sia32Object::with_sections(Vec::new(), Vec::new(), 41u32.to_le_bytes().to_vec());
    data.define_section_symbol("counter", Sia32Section::Data, 0)
        .expect("counter");

    let image = build_sia32_flat_image(&[caller, callee, data], 0, "entry", 16)
        .expect("link executable image");
    assert_eq!(image.entry(), 0, "SIA32-I Cpu starts at address zero");

    let counter_address = image.layout().data.address;
    let mut cpu = Cpu::new(image.bytes(), 4096, false, DEFAULT_MAX_STEPS).expect("load image");
    let stop = cpu.run().expect("execute linked image");

    assert_eq!(stop, Stop::Exit(42));
    assert_eq!(cpu.read_reg(1), 42);
    assert_eq!(cpu.read_word(counter_address).expect("counter memory"), 42);
}
