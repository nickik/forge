use forge_fir::{ContextSlot, DefId, FirGlobal, FirInstructionKind, FirModule, IntWidth, Ty};

use crate::BackendError;

pub(crate) fn install_context_storage(module: &mut FirModule) -> Result<(), BackendError> {
    if !module_uses_context(module) {
        return Ok(());
    }

    let mut used = module
        .functions
        .keys()
        .chain(module.globals.keys())
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    let mut next = u32::MAX;
    for slot in context_slots() {
        let owner = loop {
            let candidate = DefId(next);
            if used.insert(candidate) {
                break candidate;
            }
            if next == 0 {
                return Err(shape("no DefId remains for native execution-context storage"));
            }
            next -= 1;
        };
        if next > 0 {
            next -= 1;
        }
        module.globals.insert(
            owner,
            FirGlobal {
                owner,
                ty: storage_marker_type(slot),
                constant: None,
            },
        );
    }
    Ok(())
}

pub(crate) fn storage_owner(
    globals: &std::collections::BTreeMap<DefId, FirGlobal>,
    slot: ContextSlot,
) -> Result<DefId, BackendError> {
    let marker = storage_marker_type(slot);
    let mut matches = globals
        .iter()
        .filter_map(|(owner, global)| (global.ty == marker && global.constant.is_none()).then_some(*owner));
    let owner = matches
        .next()
        .ok_or_else(|| shape(format!("missing native storage for context slot {slot:?}")))?;
    if matches.next().is_some() {
        return Err(shape(format!(
            "ambiguous native storage marker for context slot {slot:?}"
        )));
    }
    Ok(owner)
}

fn module_uses_context(module: &FirModule) -> bool {
    fn function_uses_context(function: &forge_fir::FirFunction) -> bool {
        function.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction.kind,
                    FirInstructionKind::ContextLoad { .. }
                        | FirInstructionKind::ContextSave { .. }
                        | FirInstructionKind::ContextSet { .. }
                        | FirInstructionKind::ContextRestore { .. }
                )
            })
        })
    }

    module.functions.values().any(function_uses_context)
        || module
            .global_initializers
            .values()
            .any(|initializer| function_uses_context(&initializer.function))
}

fn context_slots() -> [ContextSlot; 5] {
    [
        ContextSlot::Scratch,
        ContextSlot::Logger,
        ContextSlot::Clock,
        ContextSlot::Random,
        ContextSlot::Trace,
    ]
}

/// C14 keeps execution-context storage private to the backend. The nested raw
/// pointer shape is only a collision-resistant marker used to recover the
/// synthetic zero-fill global for a semantic slot; every marker still has one
/// machine-pointer layout and is never visible to Forge source.
fn storage_marker_type(slot: ContextSlot) -> Ty {
    let depth = match slot {
        ContextSlot::Scratch => 1,
        ContextSlot::Logger => 2,
        ContextSlot::Clock => 3,
        ContextSlot::Random => 4,
        ContextSlot::Trace => 5,
    };
    let mut ty = Ty::Int {
        signed: false,
        width: IntWidth::W8,
    };
    for _ in 0..depth {
        ty = Ty::Pointer {
            volatile: true,
            inner: Box::new(ty),
        };
    }
    ty
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
