#![cfg(all(target_arch = "aarch64", target_os = "linux"))]

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::ptr;

use forge_codegen_cranelift::CraneliftBackend;
use forge_fir::{
    BinaryOp, DefId, FirBasicBlock, FirBlockId, FirConst, FirFunction, FirInstruction,
    FirInstructionKind, FirLocal, FirLocalId, FirModule, FirPlace, FirTerminator, FirValueId,
    IntWidth, OverflowMode, Span, Ty,
};

fn u64_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::W64,
    }
}

fn add_module() -> (FirModule, DefId) {
    let owner = DefId(0);
    let param = FirLocalId(0);
    let span = Span::new(0, 0);
    let ty = u64_ty();
    let v0 = FirValueId(0);
    let v1 = FirValueId(1);
    let v2 = FirValueId(2);

    let function = FirFunction {
        owner,
        params: vec![param],
        return_type: ty.clone(),
        locals: BTreeMap::from([(
            param,
            FirLocal {
                id: param,
                source: None,
                ty: ty.clone(),
                mutable: false,
                parameter: true,
                synthetic: false,
            },
        )]),
        closures: BTreeMap::new(),
        entry: FirBlockId(0),
        blocks: vec![FirBasicBlock {
            id: FirBlockId(0),
            closure: None,
            instructions: vec![
                FirInstruction {
                    span,
                    result: Some(v0),
                    kind: FirInstructionKind::Load {
                        place: FirPlace::Local { local: param },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v1),
                    kind: FirInstructionKind::Const {
                        value: FirConst::Integer { text: "7".into() },
                    },
                },
                FirInstruction {
                    span,
                    result: Some(v2),
                    kind: FirInstructionKind::Binary {
                        op: BinaryOp::Add,
                        overflow: Some(OverflowMode::Wrapping),
                        left: v0,
                        right: v1,
                    },
                },
            ],
            terminator: Some(FirTerminator::Return { value: Some(v2) }),
        }],
        value_types: BTreeMap::from([(v0, ty.clone()), (v1, ty.clone()), (v2, ty)]),
    };

    let mut module = FirModule::default();
    module.functions.insert(owner, function);
    (module, owner)
}

#[test]
fn executes_scalar_fir_as_native_aarch64_machine_code() {
    let backend = CraneliftBackend::aarch64().expect("AArch64 backend");
    let (module, owner) = add_module();
    let prepared = backend.prepare_module(&module).expect("verified CLIF");
    let machine = backend
        .emit_machine_code(&prepared, owner)
        .expect("AArch64 machine code");

    assert_eq!(machine.owner(), owner);
    assert!(!machine.bytes().is_empty());

    let executable = ExecutableMemory::new(machine.bytes());
    let add7: extern "C" fn(u64) -> u64 = unsafe { std::mem::transmute(executable.ptr) };
    assert_eq!(add7(0), 7);
    assert_eq!(add7(5), 12);
    assert_eq!(add7(u64::MAX), 6);
}

struct ExecutableMemory {
    ptr: *mut c_void,
    len: usize,
}

impl ExecutableMemory {
    fn new(code: &[u8]) -> Self {
        let len = code.len();
        let ptr = unsafe {
            mmap(
                ptr::null_mut(),
                len,
                PROT_READ | PROT_WRITE,
                MAP_PRIVATE | MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        assert_ne!(ptr, MAP_FAILED, "mmap failed");
        unsafe {
            ptr::copy_nonoverlapping(code.as_ptr(), ptr.cast::<u8>(), code.len());
            flush_icache(ptr.cast::<u8>(), code.len());
            assert_eq!(mprotect(ptr, len, PROT_READ | PROT_EXEC), 0, "mprotect failed");
        }
        Self { ptr, len }
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        unsafe {
            let _ = munmap(self.ptr, self.len);
        }
    }
}

unsafe fn flush_icache(start: *mut u8, len: usize) {
    use core::arch::asm;

    let ctr: u64;
    unsafe {
        asm!("mrs {ctr}, ctr_el0", ctr = out(reg) ctr, options(nomem, nostack, preserves_flags));
    }
    let dcache_line = 4usize << ((ctr >> 16) & 0xf);
    let icache_line = 4usize << (ctr & 0xf);
    let end = start as usize + len;

    let mut address = (start as usize) & !(dcache_line - 1);
    while address < end {
        unsafe {
            asm!("dc cvau, {address}", address = in(reg) address, options(nostack, preserves_flags));
        }
        address += dcache_line;
    }
    unsafe {
        asm!("dsb ish", options(nostack, preserves_flags));
    }

    address = (start as usize) & !(icache_line - 1);
    while address < end {
        unsafe {
            asm!("ic ivau, {address}", address = in(reg) address, options(nostack, preserves_flags));
        }
        address += icache_line;
    }
    unsafe {
        asm!("dsb ish", "isb", options(nostack, preserves_flags));
    }
}

const PROT_READ: i32 = 0x1;
const PROT_WRITE: i32 = 0x2;
const PROT_EXEC: i32 = 0x4;
const MAP_PRIVATE: i32 = 0x02;
const MAP_ANONYMOUS: i32 = 0x20;
const MAP_FAILED: *mut c_void = usize::MAX as *mut c_void;

unsafe extern "C" {
    fn mmap(
        address: *mut c_void,
        length: usize,
        protection: i32,
        flags: i32,
        fd: i32,
        offset: isize,
    ) -> *mut c_void;
    fn mprotect(address: *mut c_void, length: usize, protection: i32) -> i32;
    fn munmap(address: *mut c_void, length: usize) -> i32;
}
