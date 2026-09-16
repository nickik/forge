use std::collections::{BTreeMap, BTreeSet};

use forge_fir::{
    DefId, ExprId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,
    FirLocal, FirLocalId, FirSelectCase, FirTerminator, FirValueId, Ty,
};

use crate::BackendError;

/// Lift non-escaping captured closures into deterministic synthetic module
/// functions using the C14 environment-pointer ABI.
///
/// The caller retains lexical ownership of the stack environment. The lifted
/// function receives that environment pointer as its hidden first parameter;
/// returning a closure or storing one globally remains rejected until Forge has
/// an explicit owned-callable allocation/lifetime model.
pub(crate) fn lift_captured_closure_values(
    functions: &mut BTreeMap<DefId, FirFunction>,
    used: &mut BTreeSet<DefId>,
    next_internal: &mut u32,
) -> Result<(), BackendError> {
    let originals = functions.values().cloned().collect::<Vec<_>>();
    let mut additions = Vec::new();

    for original in originals {
        if matches!(original.return_type, Ty::Closure { .. }) {
            return Err(BackendError::UnsupportedFir {
                component: "escaping closure return requires heap/lifetime support",
            });
        }

        let candidates = original
            .closures
            .iter()
            .filter_map(|(id, closure)| {
                (!closure.function_pointer && !closure.captures.is_empty()).then_some(*id)
            })
            .collect::<Vec<_>>();

        for closure_id in candidates {
            let closure = original
                .closures
                .get(&closure_id)
                .ok_or_else(|| shape("captured closure metadata disappeared"))?;
            if matches!(closure.return_type, Ty::Closure { .. }) {
                return Err(BackendError::UnsupportedFir {
                    component: "escaping captured closure return requires heap/lifetime support",
                });
            }

            let owner = allocate_internal_owner(used, next_internal)?;
            let lifted = lift_one_captured_closure(&original, closure_id, owner)?;
            let code_ty = captured_closure_code_type(&original, closure_id)?;
            additions.push((owner, lifted));

            let rewritten = functions.get_mut(&original.owner).ok_or_else(|| {
                shape("enclosing function disappeared during captured closure lifting")
            })?;
            let mut next_value = rewritten
                .value_types
                .keys()
                .map(|id| id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| shape("captured closure value id overflow"))?;
            let mut rewrites = 0usize;

            for block in &mut rewritten.blocks {
                let mut materialized = Vec::with_capacity(block.instructions.len() + 1);
                for mut instruction in std::mem::take(&mut block.instructions) {
                    let matches = matches!(
                        &instruction.kind,
                        FirInstructionKind::MakeClosure { closure, .. } if *closure == closure_id
                    );
                    if matches {
                        let code = FirValueId(next_value);
                        next_value = next_value
                            .checked_add(1)
                            .ok_or_else(|| shape("captured closure value id overflow"))?;
                        rewritten.value_types.insert(code, code_ty.clone());
                        materialized.push(FirInstruction {
                            span: instruction.span,
                            result: Some(code),
                            kind: FirInstructionKind::FunctionRef { target: owner },
                        });
                        let FirInstructionKind::MakeClosure { captures, .. } =
                            &mut instruction.kind
                        else {
                            unreachable!();
                        };
                        captures.insert(0, code);
                        rewrites += 1;
                    }
                    materialized.push(instruction);
                }
                block.instructions = materialized;
            }

            if rewrites == 0 {
                return Err(shape(format!(
                    "captured closure {closure_id:?} has no construction site"
                )));
            }
        }
    }

    for (owner, function) in additions {
        if functions.insert(owner, function).is_some() {
            return Err(shape(format!(
                "synthetic captured closure owner {owner:?} collided"
            )));
        }
    }
    Ok(())
}

fn captured_closure_signature(
    enclosing: &FirFunction,
    closure_id: ExprId,
) -> Result<(Ty, Vec<Ty>), BackendError> {
    let closure = enclosing
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("missing captured closure metadata"))?;
    let params = closure
        .params
        .iter()
        .map(|local| {
            enclosing
                .locals
                .get(local)
                .map(|local| local.ty.clone())
                .ok_or_else(|| shape(format!("missing captured closure parameter {local:?}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let closure_ty = Ty::Closure {
        params: params.clone(),
        result: Box::new(closure.return_type.clone()),
    };
    Ok((closure_ty, params))
}

fn captured_closure_code_type(
    enclosing: &FirFunction,
    closure_id: ExprId,
) -> Result<Ty, BackendError> {
    let closure = enclosing
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("missing captured closure metadata"))?;
    let (closure_ty, params) = captured_closure_signature(enclosing, closure_id)?;
    let mut code_params = Vec::with_capacity(params.len() + 1);
    code_params.push(closure_ty);
    code_params.extend(params);
    Ok(Ty::Function {
        params: code_params,
        result: Box::new(closure.return_type.clone()),
        named_arguments: false,
    })
}

fn lift_one_captured_closure(
    enclosing: &FirFunction,
    closure_id: ExprId,
    owner: DefId,
) -> Result<FirFunction, BackendError> {
    let closure = enclosing
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("missing captured closure metadata while lifting"))?
        .clone();
    let blocks = enclosing
        .blocks
        .iter()
        .filter(|block| block.closure == Some(closure_id))
        .cloned()
        .collect::<Vec<_>>();
    if blocks.is_empty() {
        return Err(shape("captured closure has no FIR blocks"));
    }
    if !blocks.iter().any(|block| block.id == closure.entry) {
        return Err(shape("captured closure entry block is missing"));
    }

    let env_id = enclosing
        .locals
        .keys()
        .map(|id| id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .map(FirLocalId)
        .ok_or_else(|| shape("captured closure hidden environment local overflow"))?;
    let (closure_ty, _) = captured_closure_signature(enclosing, closure_id)?;

    let mut locals = enclosing.locals.clone();
    for local in locals.values_mut() {
        local.parameter = false;
    }
    locals.insert(
        env_id,
        FirLocal {
            id: env_id,
            source: None,
            ty: closure_ty,
            mutable: false,
            parameter: true,
            synthetic: true,
        },
    );
    for parameter in &closure.params {
        let local = locals.get_mut(parameter).ok_or_else(|| {
            shape(format!(
                "missing lifted captured closure parameter {parameter:?}"
            ))
        })?;
        local.parameter = true;
    }

    let mut params = Vec::with_capacity(closure.params.len() + 1);
    params.push(env_id);
    params.extend(closure.params.iter().copied());
    let mut closures = BTreeMap::new();
    closures.insert(closure_id, closure.clone());

    Ok(FirFunction {
        owner,
        params,
        return_type: closure.return_type.clone(),
        locals,
        closures,
        entry: closure.entry,
        blocks,
        value_types: enclosing.value_types.clone(),
    })
}

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
