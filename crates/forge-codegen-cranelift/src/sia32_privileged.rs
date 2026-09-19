//! SIA32-P privileged instruction encoding surface for freestanding Forge code.
//!
//! This module deliberately owns only the architectural 16-bit encodings.  It
//! does not invent a second execution model: semantics remain those of SIA32-P
//! and the Cranelift SIA32 backend / Lighting implementation.

pub const SYSREG_STATUS: u8 = 0x0;
pub const SYSREG_EPC: u8 = 0x1;
pub const SYSREG_CAUSE: u8 = 0x2;
pub const SYSREG_BADADDR: u8 = 0x3;
pub const SYSREG_SCRATCH: u8 = 0x4;
pub const SYSREG_VMCTX: u8 = 0x5;

const fn reg(reg: u8) -> u16 {
    assert!(reg < 16);
    reg as u16
}

const fn sysreg(sr: u8) -> u16 {
    assert!(sr < 6);
    sr as u16
}

/// F0 rd sr
pub const fn sread(rd: u8, sr: u8) -> u16 {
    0xf000 | (reg(rd) << 4) | sysreg(sr)
}

/// F1 rs sr
pub const fn swrite(sr: u8, rs: u8) -> u16 {
    0xf100 | (reg(rs) << 4) | sysreg(sr)
}

/// F2 rg 4 -- baseline SSWAP is defined only for SCRATCH.
pub const fn sswap_scratch(rg: u8) -> u16 {
    0xf200 | (reg(rg) << 4) | SYSREG_SCRATCH as u16
}

pub const SRET: u16 = 0xf300;

pub const fn sretctx(rs: u8) -> u16 {
    0xf301 | (reg(rs) << 4)
}

pub const TLBFENCE: u16 = 0xf400;

pub const fn tlbfence_va(rs: u8) -> u16 {
    0xf401 | (reg(rs) << 4)
}

pub const fn tlbfence_asid(rs: u8) -> u16 {
    0xf402 | (reg(rs) << 4)
}

pub const WFI: u16 = 0xf500;
pub const SYNC_I: u16 = 0xf600;
pub const FENCE: u16 = 0xf700;

/// Frozen SIA32-I architectural TRAP imm8 encoding.
pub const fn trap(imm8: u8) -> u16 {
    0xc00f | ((imm8 as u16) << 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_sia32_p_encoding_examples() {
        assert_eq!(sread(3, SYSREG_EPC), 0xf031);
        assert_eq!(sread(3, SYSREG_CAUSE), 0xf032);
        assert_eq!(swrite(SYSREG_EPC, 3), 0xf131);
        assert_eq!(swrite(SYSREG_VMCTX, 3), 0xf135);
        assert_eq!(sswap_scratch(13), 0xf2d4);
        assert_eq!(SRET, 0xf300);
        assert_eq!(sretctx(8), 0xf381);
        assert_eq!(TLBFENCE, 0xf400);
        assert_eq!(tlbfence_va(1), 0xf411);
        assert_eq!(tlbfence_asid(2), 0xf422);
        assert_eq!(WFI, 0xf500);
        assert_eq!(SYNC_I, 0xf600);
        assert_eq!(FENCE, 0xf700);
        assert_eq!(trap(0x27), 0xc27f);
    }
}
