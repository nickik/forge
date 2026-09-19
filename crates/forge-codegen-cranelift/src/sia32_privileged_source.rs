//! Freestanding Forge SIA32-P source contract.
//!
//! These names are the source-level operations Cosmic needs for protected
//! bring-up.  The parser/frontend wiring is intentionally kept separate from
//! the architectural encoding module so non-SIA targets can reject them
//! explicitly rather than silently assigning host semantics.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sia32PrivilegedOp {
    Trap { imm8: u8 },
    ReadSystem { rd: u8, system_register: u8 },
    WriteSystem { system_register: u8, rs: u8 },
    SwapScratch { register: u8 },
    Return,
    ReturnContext { rs: u8 },
    TlbFence,
    TlbFenceVa { rs: u8 },
    TlbFenceAsid { rs: u8 },
    WaitForInterrupt,
    SyncInstruction,
    Fence,
}

impl Sia32PrivilegedOp {
    pub const fn encode(self) -> u16 {
        use crate::sia32_privileged as p;
        match self {
            Self::Trap { imm8 } => p::trap(imm8),
            Self::ReadSystem { rd, system_register } => p::sread(rd, system_register),
            Self::WriteSystem { system_register, rs } => p::swrite(system_register, rs),
            Self::SwapScratch { register } => p::sswap_scratch(register),
            Self::Return => p::SRET,
            Self::ReturnContext { rs } => p::sretctx(rs),
            Self::TlbFence => p::TLBFENCE,
            Self::TlbFenceVa { rs } => p::tlbfence_va(rs),
            Self::TlbFenceAsid { rs } => p::tlbfence_asid(rs),
            Self::WaitForInterrupt => p::WFI,
            Self::SyncInstruction => p::SYNC_I,
            Self::Fence => p::FENCE,
        }
    }
}

/// Stable source spellings reserved for the Forge freestanding SIA module.
/// Frontend lowering should resolve these only when the selected target is SIA32.
pub mod source_name {
    pub const TRAP: &str = "sia_trap";
    pub const SREAD: &str = "sia_sread";
    pub const SWRITE: &str = "sia_swrite";
    pub const SSWAP_SCRATCH: &str = "sia_sswap_scratch";
    pub const SRET: &str = "sia_sret";
    pub const SRETCTX: &str = "sia_sretctx";
    pub const TLBFENCE: &str = "sia_tlbfence";
    pub const TLBFENCE_VA: &str = "sia_tlbfence_va";
    pub const TLBFENCE_ASID: &str = "sia_tlbfence_asid";
    pub const WFI: &str = "sia_wfi";
    pub const SYNC_I: &str = "sia_sync_i";
    pub const FENCE: &str = "sia_fence";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sia32_privileged::*;

    #[test]
    fn source_contract_maps_only_to_normative_encodings() {
        assert_eq!(Sia32PrivilegedOp::Trap { imm8: 0x27 }.encode(), trap(0x27));
        assert_eq!(Sia32PrivilegedOp::ReadSystem { rd: 3, system_register: SYSREG_CAUSE }.encode(), 0xf032);
        assert_eq!(Sia32PrivilegedOp::WriteSystem { system_register: SYSREG_VMCTX, rs: 3 }.encode(), 0xf135);
        assert_eq!(Sia32PrivilegedOp::Return.encode(), SRET);
        assert_eq!(Sia32PrivilegedOp::TlbFence.encode(), TLBFENCE);
        assert_eq!(Sia32PrivilegedOp::WaitForInterrupt.encode(), WFI);
    }

    #[test]
    fn cosmic_required_source_names_are_frozen() {
        assert_eq!(source_name::TRAP, "sia_trap");
        assert_eq!(source_name::SREAD, "sia_sread");
        assert_eq!(source_name::SWRITE, "sia_swrite");
        assert_eq!(source_name::SRET, "sia_sret");
        assert_eq!(source_name::SRETCTX, "sia_sretctx");
        assert_eq!(source_name::TLBFENCE, "sia_tlbfence");
    }
}
