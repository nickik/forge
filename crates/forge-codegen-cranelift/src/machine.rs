use cranelift_codegen::control::ControlPlane;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::TargetIsa;
use cranelift_codegen::Context;
use forge_fir::DefId;

use crate::{BackendError, CraneliftBackend, CraneliftTarget, PreparedModule};

/// Relocation-free machine code emitted for one verified FIR function.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineCode {
    target: CraneliftTarget,
    owner: DefId,
    bytes: Vec<u8>,
}

impl MachineCode {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl CraneliftBackend {
    /// Compile one already-prepared function to target machine code.
    /// C5 deliberately rejects relocations until calls/globals have a linker contract.
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

        let function = prepared
            .function(owner)
            .ok_or_else(|| BackendError::InvalidFirShape {
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
    let compiled = context
        .compile(isa, &mut control)
        .map_err(|error| BackendError::Cranelift {
            message: format!("machine-code compilation failed for {owner:?}: {error}"),
        })?;

    if !compiled.buffer.relocs().is_empty() {
        return Err(BackendError::UnsupportedFir {
            component: "machine-code relocations",
        });
    }

    let bytes = compiled.code_buffer().to_vec();
    if bytes.is_empty() {
        return Err(BackendError::Cranelift {
            message: format!("machine-code compilation produced no bytes for {owner:?}"),
        });
    }

    Ok(MachineCode {
        target,
        owner,
        bytes,
    })
}
