use std::collections::{BTreeMap, BTreeSet};

use forge_fir::{
    verify_fir_module, ConstValue, DefId, FirModule, Layout, LayoutEngine, LayoutTarget, Ty,
    TypeDefinitionTable,
};

use crate::object::ObjectLinkage;
use crate::{BackendError, CraneliftBackend, CraneliftTarget};

/// How a Forge global obtains its initial value before ordinary program code
/// observes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GlobalInitialization {
    /// The frontend proved a scalar value at compile time. C11b will serialize
    /// it using the authoritative C9 layout and place it in read-only storage.
    Constant(ConstValue),
    /// Storage begins zeroed and C11d will execute the FIR initializer in the
    /// already-verified dependency order.
    Runtime {
        dependencies: Vec<DefId>,
        order: u32,
    },
    /// No explicit initializer exists. The object representation is zero-fill.
    Zero,
}

/// Intended native-object storage class. C11a records policy only; C11b owns
/// the actual ELF section construction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobalStorageClass {
    ReadOnlyData,
    /// Reserved for writable compile-time data once FIR exposes such a form.
    /// Current FIR constants are immutable and runtime-initialized globals
    /// begin in zero-fill storage.
    WritableData,
    ZeroFill,
}

/// C9-layout-complete representation of one FIR global before object bytes are
/// emitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedGlobal {
    owner: DefId,
    ty: Ty,
    layout: Layout,
    initialization: GlobalInitialization,
    storage: GlobalStorageClass,
}

impl PreparedGlobal {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn ty(&self) -> &Ty {
        &self.ty
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn initialization(&self) -> &GlobalInitialization {
        &self.initialization
    }

    pub const fn storage(&self) -> GlobalStorageClass {
        self.storage
    }
}

/// Prepared global state for a verified FIR module. The map and initializer
/// order are deterministic because FIR owns `DefId` keys and explicit init
/// ordering.
#[derive(Clone, Debug)]
pub struct PreparedGlobals {
    target: CraneliftTarget,
    globals: BTreeMap<DefId, PreparedGlobal>,
    init_order: Vec<DefId>,
}

impl PreparedGlobals {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn globals(&self) -> &BTreeMap<DefId, PreparedGlobal> {
        &self.globals
    }

    pub fn global(&self, owner: DefId) -> Option<&PreparedGlobal> {
        self.globals.get(&owner)
    }

    pub fn init_order(&self) -> &[DefId] {
        &self.init_order
    }
}

/// Deterministic object-symbol metadata for one prepared global. Actual ELF
/// section indices and byte offsets deliberately remain C11b concerns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalObjectSymbol {
    owner: DefId,
    name: String,
    linkage: ObjectLinkage,
    layout: Layout,
    initialization: GlobalInitialization,
    storage: GlobalStorageClass,
}

impl GlobalObjectSymbol {
    pub const fn owner(&self) -> DefId {
        self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub const fn linkage(&self) -> ObjectLinkage {
        self.linkage
    }

    pub fn layout(&self) -> &Layout {
        &self.layout
    }

    pub fn initialization(&self) -> &GlobalInitialization {
        &self.initialization
    }

    pub const fn storage(&self) -> GlobalStorageClass {
        self.storage
    }
}

#[derive(Clone, Debug)]
pub struct GlobalObjectPlan {
    target: CraneliftTarget,
    symbols: BTreeMap<DefId, GlobalObjectSymbol>,
}

impl GlobalObjectPlan {
    pub const fn target(&self) -> CraneliftTarget {
        self.target
    }

    pub fn symbols(&self) -> &BTreeMap<DefId, GlobalObjectSymbol> {
        &self.symbols
    }

    pub fn symbol(&self, owner: DefId) -> Option<&GlobalObjectSymbol> {
        self.symbols.get(&owner)
    }
}

impl CraneliftBackend {
    /// Prepare all global layout/initialization metadata without emitting data
    /// sections or lowering `LoadGlobal` yet.
    pub fn prepare_globals(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<PreparedGlobals, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        for owner in module.globals.keys() {
            if module.functions.contains_key(owner) {
                return Err(shape(format!(
                    "definition {owner:?} appears as both a function and a global"
                )));
            }
        }

        let positions: BTreeMap<DefId, u32> = module
            .global_init_order
            .iter()
            .copied()
            .enumerate()
            .map(|(index, owner)| {
                u32::try_from(index)
                    .map(|index| (owner, index))
                    .map_err(|_| shape("global initializer order exceeds u32"))
            })
            .collect::<Result<_, _>>()?;

        let mut layout_engine = LayoutEngine::new(
            LayoutTarget::new(self.target().layout().pointer_bits),
            definitions,
        );
        let mut globals = BTreeMap::new();

        for (owner, global) in &module.globals {
            let initializer = module.global_initializers.get(owner);
            if global.constant.is_some() && initializer.is_some() {
                return Err(shape(format!(
                    "global {owner:?} has both a compile-time value and a runtime initializer"
                )));
            }

            let layout = layout_engine.layout_of(&global.ty).map_err(|error| {
                shape(format!(
                    "global {owner:?} has invalid C9 layout for {:?}: {error}",
                    global.ty
                ))
            })?;

            let (initialization, storage) = match (&global.constant, initializer) {
                (Some(constant), None) => {
                    validate_constant_type(*owner, constant, &global.ty)?;
                    (
                        GlobalInitialization::Constant(constant.clone()),
                        GlobalStorageClass::ReadOnlyData,
                    )
                }
                (None, Some(initializer)) => {
                    let order = positions.get(owner).copied().ok_or_else(|| {
                        shape(format!(
                            "runtime initializer for global {owner:?} is missing from global_init_order"
                        ))
                    })?;
                    (
                        GlobalInitialization::Runtime {
                            dependencies: initializer.dependencies.clone(),
                            order,
                        },
                        GlobalStorageClass::ZeroFill,
                    )
                }
                (None, None) => (GlobalInitialization::Zero, GlobalStorageClass::ZeroFill),
                (Some(_), Some(_)) => unreachable!("constant/runtime conflict rejected above"),
            };

            globals.insert(
                *owner,
                PreparedGlobal {
                    owner: *owner,
                    ty: global.ty.clone(),
                    layout,
                    initialization,
                    storage,
                },
            );
        }

        for owner in &module.global_init_order {
            if !globals.contains_key(owner) {
                return Err(shape(format!(
                    "global initializer order contains missing global {owner:?}"
                )));
            }
        }

        Ok(PreparedGlobals {
            target: self.target(),
            globals,
            init_order: module.global_init_order.clone(),
        })
    }

    pub fn plan_global_objects(
        &self,
        prepared: &PreparedGlobals,
    ) -> Result<GlobalObjectPlan, BackendError> {
        self.plan_global_objects_with_exports(prepared, std::iter::empty())
    }

    pub fn plan_global_objects_with_exports<I>(
        &self,
        prepared: &PreparedGlobals,
        exports: I,
    ) -> Result<GlobalObjectPlan, BackendError>
    where
        I: IntoIterator<Item = DefId>,
    {
        if prepared.target() != self.target() {
            return Err(shape(format!(
                "prepared globals target {:?} does not match object-plan target {:?}",
                prepared.target(),
                self.target()
            )));
        }

        let exports: BTreeSet<DefId> = exports.into_iter().collect();
        for owner in &exports {
            if !prepared.globals().contains_key(owner) {
                return Err(shape(format!("cannot export missing FIR global {owner:?}")));
            }
        }

        let mut names = BTreeSet::new();
        let mut symbols = BTreeMap::new();
        for (owner, global) in prepared.globals() {
            let name = forge_global_symbol(*owner);
            if !names.insert(name.clone()) {
                return Err(shape(format!(
                    "duplicate Forge global object symbol generated for {owner:?}: {name}"
                )));
            }
            symbols.insert(
                *owner,
                GlobalObjectSymbol {
                    owner: *owner,
                    name,
                    linkage: if exports.contains(owner) {
                        ObjectLinkage::Export
                    } else {
                        ObjectLinkage::Local
                    },
                    layout: global.layout().clone(),
                    initialization: global.initialization().clone(),
                    storage: global.storage(),
                },
            );
        }

        Ok(GlobalObjectPlan {
            target: self.target(),
            symbols,
        })
    }
}

fn validate_constant_type(
    owner: DefId,
    constant: &ConstValue,
    ty: &Ty,
) -> Result<(), BackendError> {
    let valid = match constant {
        ConstValue::Integer { .. } => matches!(ty, Ty::Byte | Ty::Int { .. }),
        ConstValue::Bool { .. } => matches!(ty, Ty::Bool),
        ConstValue::Char { .. } => matches!(ty, Ty::Char),
    };
    if valid {
        Ok(())
    } else {
        Err(shape(format!(
            "global {owner:?} constant {constant:?} does not match FIR type {ty:?}"
        )))
    }
}

fn forge_global_symbol(owner: DefId) -> String {
    format!("__forge_global_{:08x}", owner.0)
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
