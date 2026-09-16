use crate::{
    build_sia32_flat_image, BackendError, CraneliftTarget, ExecutableFormat, Sia32ExecutableImage,
    Sia32Object, TargetAbi, TargetLayout,
};

/// Forge-side SIA32 integration boundary available before general CLIF lowering.
/// It owns target policy and delegates object serialization/link/image creation
/// to the real M8 implementation rather than a parallel test-only path.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sia32IntegrationShell;

impl Sia32IntegrationShell {
    pub const fn target(self) -> CraneliftTarget {
        CraneliftTarget::Sia32
    }

    pub const fn triple(self) -> &'static str {
        CraneliftTarget::Sia32.triple()
    }

    pub const fn layout(self) -> TargetLayout {
        CraneliftTarget::Sia32.layout()
    }

    pub const fn pointer_bits(self) -> u16 {
        self.layout().pointer_bits
    }

    pub const fn abi(self) -> TargetAbi {
        CraneliftTarget::Sia32.abi()
    }

    pub const fn executable_format(self) -> ExecutableFormat {
        CraneliftTarget::Sia32.executable_format()
    }

    /// Prove that target selection reaches the registered Crainlift SIA32
    /// backend without compiling a function.
    pub fn validate_target_registration(self) -> Result<(), BackendError> {
        let isa = CraneliftTarget::Sia32.isa()?;
        if isa.name() != "sia32" {
            return Err(BackendError::InvalidTarget {
                triple: CraneliftTarget::Sia32.triple(),
                message: format!("target lookup resolved to {}", isa.name()),
            });
        }
        if isa.pointer_type().bits() != 32 {
            return Err(BackendError::InvalidTarget {
                triple: CraneliftTarget::Sia32.triple(),
                message: format!("target reports {}-bit pointers", isa.pointer_type().bits()),
            });
        }
        Ok(())
    }

    /// Serialize through the M8 SIAO32 object writer.
    pub fn write_object(self, object: &Sia32Object) -> Result<Vec<u8>, BackendError> {
        object.to_bytes()
    }

    /// Parse through the same strict M8 object contract.
    pub fn read_object(self, bytes: &[u8]) -> Result<Sia32Object, BackendError> {
        Sia32Object::from_bytes(bytes)
    }

    /// Link and lay out a loadable SIA32 flat image through the M8.3 builder.
    pub fn build_image(
        self,
        objects: &[Sia32Object],
        load_address: u32,
        entry_symbol: &str,
        bss_size: u32,
    ) -> Result<Sia32ExecutableImage, BackendError> {
        build_sia32_flat_image(objects, load_address, entry_symbol, bss_size)
    }

    /// General Forge FIR -> CLIF -> SIA32 compilation remains an explicit
    /// boundary until M5 lowering is completed.
    pub fn require_clif_lowering(self) -> Result<(), BackendError> {
        Err(BackendError::UnfinishedTargetLowering { target: "SIA32" })
    }
}
