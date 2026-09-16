use std::collections::{BTreeMap, BTreeSet};

use forge_fir::{
    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstructionKind, FirSelectCase, FirTerminator,
};

use crate::BackendError;

/// Lift capture-free anonymous functions marked as first-class `fn(...)`
/// values into deterministic synthetic module functions.
///
/// The frontend/FIR has already proved that `function_pointer` closures have
/// no environment captures. Native codegen therefore does not need a closure
/// ABI: the value is an ordinary function address and calls use the existing
/// C9 indirect-function ABI.
pub(crate) fn lift_capture_free_function_values(
    functions: &mut BTreeMap<DefId, FirFunction>,
    used: &mut BTreeSet<DefId>,
    next_internal: &mut u32,
) -> Result<(), BackendError> {
    let originals = functions.values().cloned().collect::<Vec<_>>();
    let mut additions = Vec::new();

    for original in originals {
        let candidates = original
            .closures
            .iter()
            .filter_map(|(id, closure)| closure.function_pointer.then_some(*id))
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            continue;
        }

        for closure_id in candidates {
            let closure = original
                .closures
                .get(&closure_id)
                .ok_or_else(|| shape("capture-free closure metadata disappeared"))?;
            if !closure.captures.is_empty() {
                return Err(shape(format!(
                    "function-pointer closure {closure_id:?} unexpectedly has captures"
                )));
            }

            let owner = allocate_internal_owner(used, next_internal)?;
            let lifted = lift_one_capture_free_closure(&original, closure_id, owner)?;
            additions.push((owner, lifted));

            let rewritten = functions
                .get_mut(&original.owner)
                .ok_or_else(|| shape("enclosing function disappeared during closure lifting"))?;
            let rewritten_closure = rewritten
                .closures
                .get_mut(&closure_id)
                .ok_or_else(|| shape("rewritten closure metadata disappeared"))?;

            // Keep the closure blocks/metadata until the ordinary C14 CFG pass:
            // it already excludes closure-tagged blocks from the enclosing CFG.
            // Marking this false prevents the old intentional rejection while
            // avoiding block-ID surgery in the already-verified FIR.
            rewritten_closure.function_pointer = false;

            let mut rewrites = 0usize;
            for block in &mut rewritten.blocks {
                for instruction in &mut block.instructions {
                    if let FirInstructionKind::MakeClosure { closure, captures } = &instruction.kind
                    {
                        if *closure != closure_id {
                            continue;
                        }
                        if !captures.is_empty() {
                            return Err(shape(
                                "capture-free function-pointer construction has captures",
                            ));
                        }
                        instruction.kind = FirInstructionKind::FunctionRef { target: owner };
                        rewrites += 1;
                    }
                }
            }
            if rewrites == 0 {
                return Err(shape(format!(
                    "function-pointer closure {closure_id:?} has no construction site"
                )));
            }
        }
    }

    for (owner, function) in additions {
        if functions.insert(owner, function).is_some() {
            return Err(shape(format!(
                "synthetic capture-free closure owner {owner:?} collided"
            )));
        }
    }
    Ok(())
}

fn lift_one_capture_free_closure(
    enclosing: &FirFunction,
    closure_id: forge_fir::ExprId,
    owner: DefId,
) -> Result<FirFunction, BackendError> {
    let closure = enclosing
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("missing closure metadata while lifting function value"))?;
    let source_blocks = enclosing
        .blocks
        .iter()
        .filter(|block| block.closure == Some(closure_id))
        .cloned()
        .collect::<Vec<_>>();
    if source_blocks.is_empty() {
        return Err(shape("capture-free closure has no FIR blocks"));
    }

    let block_map = source_blocks
        .iter()
        .enumerate()
        .map(|(index, block)| (block.id, FirBlockId(index as u32)))
        .collect::<BTreeMap<_, _>>();
    let entry = *block_map
        .get(&closure.entry)
        .ok_or_else(|| shape("capture-free closure entry block is missing"))?;

    let mut blocks = Vec::with_capacity(source_blocks.len());
    for source in source_blocks {
        let mut block = FirBasicBlock {
            id: *block_map
                .get(&source.id)
                .ok_or_else(|| shape("missing synthetic closure block mapping"))?,
            closure: None,
            instructions: source.instructions,
            terminator: source.terminator,
        };
        if let Some(terminator) = &mut block.terminator {
            remap_terminator(terminator, &block_map)?;
        }
        blocks.push(block);
    }

    let mut locals = enclosing.locals.clone();
    for local in locals.values_mut() {
        local.parameter = false;
    }
    for parameter in &closure.params {
        let local = locals
            .get_mut(parameter)
            .ok_or_else(|| shape(format!("missing lifted closure parameter {parameter:?}")))?;
        local.parameter = true;
    }

    Ok(FirFunction {
        owner,
        params: closure.params.clone(),
        return_type: closure.return_type.clone(),
        locals,
        closures: BTreeMap::new(),
        entry,
        blocks,
        value_types: enclosing.value_types.clone(),
    })
}

fn remap_terminator(
    terminator: &mut FirTerminator,
    blocks: &BTreeMap<FirBlockId, FirBlockId>,
) -> Result<(), BackendError> {
    let remap = |target: &mut FirBlockId| -> Result<(), BackendError> {
        *target = *blocks.get(target).ok_or_else(|| {
            shape(format!(
                "capture-free closure branches outside its own CFG to {target:?}"
            ))
        })?;
        Ok(())
    };

    match terminator {
        FirTerminator::Goto { target } => remap(target),
        FirTerminator::Branch {
            then_block,
            else_block,
            ..
        } => {
            remap(then_block)?;
            remap(else_block)
        }
        FirTerminator::Select { cases, .. } => {
            for case in cases {
                match case {
                    FirSelectCase::Receive { target, .. }
                    | FirSelectCase::Timeout { target, .. } => remap(target)?,
                }
            }
            Ok(())
        }
        FirTerminator::Return { .. } | FirTerminator::Unreachable => Ok(()),
    }
}

fn allocate_internal_owner(
    used: &mut BTreeSet<DefId>,
    next: &mut u32,
) -> Result<DefId, BackendError> {
    loop {
        let candidate = DefId(*next);
        if used.insert(candidate) {
            if *next > 0 {
                *next -= 1;
            }
            return Ok(candidate);
        }
        if *next == 0 {
            return Err(shape(
                "no DefId remains for capture-free closure function values",
            ));
        }
        *next -= 1;
    }
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
