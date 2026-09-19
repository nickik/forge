use cranelift_codegen::binemit::Reloc;
use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::{ExternalName, Function};
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::RelocTarget;
use cranelift_codegen::Context;
use forge_fir::DefId;

use crate::{BackendError, CraneliftBackend, CraneliftTarget, PreparedModule};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineRelocation {
    pub offset: u32,
    pub kind: Reloc,
    pub target: DefId,
    pub addend: i64,
}

/// Machine code emitted for one verified FIR function. SIA32 calls may carry
/// relocations; the SIAO32 object/image layer resolves them before execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineCode {
    target: CraneliftTarget,
    owner: DefId,
    bytes: Vec<u8>,
    relocations: Vec<MachineRelocation>,
}

impl MachineCode {
    pub const fn target(&self) -> CraneliftTarget { self.target }
    pub const fn owner(&self) -> DefId { self.owner }
    pub fn bytes(&self) -> &[u8] { &self.bytes }
    pub fn relocations(&self) -> &[MachineRelocation] { &self.relocations }
    pub fn into_bytes(self) -> Vec<u8> { self.bytes }
}

impl CraneliftBackend {
    pub fn emit_machine_code(
        &self,
        prepared: &PreparedModule,
        owner: DefId,
    ) -> Result<MachineCode, BackendError> {
        let target = self.target();
        if prepared.target() != target {
            return Err(BackendError::InvalidFirShape {
                message: format!(
                    "prepared module target {:?} does not match backend target {:?}",
                    prepared.target(), target
                ),
            });
        }
        let function = prepared.function(owner).ok_or_else(|| BackendError::InvalidFirShape {
            message: format!("prepared module has no function {owner:?}"),
        })?;
        let isa = target.isa()?;
        compile_function(target, &*isa, owner, function)
    }
}

fn compile_function(
    target: CraneliftTarget,
    isa: &dyn TargetIsa,
    owner: DefId,
    function: &Function,
) -> Result<MachineCode, BackendError> {
    let mut context = Context::for_function(function.clone());
    let mut control = ControlPlane::default();
    let compiled = context.compile(isa, &mut control).map_err(|error| BackendError::Cranelift {
        message: format!("machine-code compilation failed for {owner:?}: {error:?}"),
    })?;

    let mut relocations = Vec::new();
    for reloc in compiled.buffer.relocs() {
        if target != CraneliftTarget::Sia32 || reloc.kind != Reloc::Abs4 {
            return Err(BackendError::UnsupportedFir { component: "machine-code relocation kind" });
        }
        let RelocTarget::ExternalName(ExternalName::User(user_ref)) = &reloc.target else {
            return Err(BackendError::UnsupportedFir { component: "non-Forge machine-code relocation target" });
        };
        let user = function.params.user_named_funcs()[*user_ref];
        if user.namespace != 0 {
            return Err(BackendError::UnsupportedFir { component: "external machine-code relocation target" });
        }
        relocations.push(MachineRelocation {
            offset: reloc.offset,
            kind: reloc.kind,
            target: DefId(user.index),
            addend: reloc.addend,
        });
    }

    let bytes = compiled.code_buffer().to_vec();
    if bytes.is_empty() {
        return Err(BackendError::Cranelift {
            message: format!("machine-code compilation produced no bytes for {owner:?}"),
        });
    }
    Ok(MachineCode { target, owner, bytes, relocations })
}
