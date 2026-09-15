use std::collections::{BTreeMap, BTreeSet};

use cranelift_codegen::ir::Signature;
use forge_fir::DefId;

use crate::{BackendError, CraneliftBackend, CraneliftTarget, PreparedModule};

/// Linkage policy at the Forge object boundary.
///
/// C10a keeps functions local unless the caller explicitly marks a FIR
/// definition for export. Source-language export semantics remain a frontend
/// concern and are not inferred from function names here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectLinkage {
    Local,
    Export,
}

/// One deterministic function symbol planned for later object emission.
#[derive(Clone, Debug)]
pub struct ObjectSymbol {
    owner: DefId,
    name: String,
    linkage: ObjectLinkage,
    signature: Signature,
}

impl ObjectSymbol {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn linkage(&self) -> ObjectLinkage {
        self.linkage
    }

    /// The exact CLIF signature produced by the C9 Forge ABI lowering.
    pub fn signature(&self) -> &Signature {
        &self.signature
    }
}

/// Deterministic symbol/linkage plan consumed by C10b object emission.
#[derive(Clone, Debug)]
pub struct ObjectModulePlan {
    target: CraneliftTarget,
    symbols: BTreeMap<DefId, ObjectSymbol>,
}

impl ObjectModulePlan {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn symbols(&self) -> &BTreeMap<DefId, ObjectSymbol> {
        &self.symbols
    }

    pub fn symbol(&self, owner: DefId) -> Option<&ObjectSymbol> {
        self.symbols.get(&owner)
    }
}

impl CraneliftBackend {
    /// Plan a module with all FIR functions kept local.
    pub fn plan_object_module(
        &self,
        prepared: &PreparedModule,
    ) -> Result<ObjectModulePlan, BackendError> {
        self.plan_object_module_with_exports(prepared, std::iter::empty())
    }

    /// Plan deterministic object symbols while explicitly promoting selected
    /// FIR definitions to exported linkage.
    pub fn plan_object_module_with_exports<I>(
        &self,
        prepared: &PreparedModule,
        exports: I,
    ) -> Result<ObjectModulePlan, BackendError>
    where
        I: IntoIterator<Item = DefId>,
    {
        if prepared.target() != self.target() {
            return Err(shape(format!(
                "prepared module target {:?} does not match object-plan target {:?}",
                prepared.target(),
                self.target()
            )));
        }

        let exports: BTreeSet<DefId> = exports.into_iter().collect();
        for owner in &exports {
            if !prepared.functions().contains_key(owner) {
                return Err(shape(format!(
                    "cannot export missing FIR function {owner:?}"
                )));
            }
        }

        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::new();
        for (owner, function) in prepared.functions() {
            let name = forge_function_symbol(*owner);
            if !names.insert(name.clone()) {
                return Err(shape(format!(
                    "duplicate Forge object symbol generated for {owner:?}: {name}"
                )));
            }
            let linkage = if exports.contains(owner) {
                ObjectLinkage::Export
            } else {
                ObjectLinkage::Local
            };
            symbols.insert(
                *owner,
                ObjectSymbol {
                    owner: *owner,
                    name,
                    linkage,
                    signature: function.signature.clone(),
                },
            );
        }

        Ok(ObjectModulePlan {
            target: self.target(),
            symbols,
        })
    }
}

fn forge_function_symbol(owner: DefId) -> String {
    format!("__forge_fn_{:08x}", owner.0)
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
