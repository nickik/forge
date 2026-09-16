from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

def read(path):
    return (ROOT / path).read_text()

def write(path, text):
    p = ROOT / path
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)

def replace_once(text, old, new, path):
    if text.count(old) != 1:
        raise SystemExit(f"{path}: expected one anchor, found {text.count(old)}: {old[:80]!r}")
    return text.replace(old, new, 1)

# Authoritative closure layout is one target pointer.
p = "crates/forge-fir/src/layout_c9a.rs"
s = read(p)
s = replace_once(s,
'''            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {\n                self.layout_pointer()\n            }''',
'''            Ty::Pointer { .. }\n            | Ty::Reference { .. }\n            | Ty::Function { .. }\n            | Ty::Closure { .. } => self.layout_pointer(),''', p)
s = replace_once(s,
'''            Ty::ContextSlot { .. } => Err(LayoutError::UnsupportedType("context slot")),\n            Ty::Closure { .. } => Err(LayoutError::UnsupportedType("closure")),''',
'''            Ty::ContextSlot { .. } => Err(LayoutError::UnsupportedType("context slot")),''', p)
write(p, s)

# ABI decomposition treats closures as pointers on both native64 and SIA32.
p = "crates/forge-fir/src/abi_c9b_impl.rs"
s = read(p)
s = replace_once(s,
'''            Ty::Pointer { .. } | Ty::Reference { .. } | Ty::Function { .. } => {\n                out.push(pointer(base, self.target.pointer_bits));\n            }''',
'''            Ty::Pointer { .. }\n            | Ty::Reference { .. }\n            | Ty::Function { .. }\n            | Ty::Closure { .. } => {\n                out.push(pointer(base, self.target.pointer_bits));\n            }''', p)
s = replace_once(s,
'''            Ty::ContextSlot { .. } => return Err(AbiError::UnsupportedType("context slot")),\n            Ty::Closure { .. } => return Err(AbiError::UnsupportedType("closure ABI")),''',
'''            Ty::ContextSlot { .. } => return Err(AbiError::UnsupportedType("context slot")),''', p)
write(p, s)

# Completion pass: allocate deterministic synthetic owners, rewrite MakeClosure
# sites to carry a code pointer, and synthesize lifted closure FIR functions.
p = "crates/forge-codegen-cranelift/src/completion.rs"
s = read(p)
s = replace_once(s,
'''use forge_fir::{\n    DefId, FirBasicBlock, FirBlockId, FirFunction, FirInstructionKind, FirSelectCase, FirTerminator,\n};''',
'''use forge_fir::{\n    DefId, ExprId, FirBasicBlock, FirBlockId, FirFunction, FirInstruction, FirInstructionKind,\n    FirLocal, FirLocalId, FirSelectCase, FirTerminator, FirValueId, Ty,\n};''', p)
insert_anchor = '''pub(crate) fn lift_capture_free_function_values(\n'''
idx = s.index(insert_anchor)
new_code = r'''pub(crate) fn lift_captured_closure_values(
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

            let rewritten = functions
                .get_mut(&original.owner)
                .ok_or_else(|| shape("enclosing function disappeared during captured closure lifting"))?;
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
                            span: instruction.span.clone(),
                            result: Some(code),
                            kind: FirInstructionKind::FunctionRef { target: owner },
                        });
                        let FirInstructionKind::MakeClosure { captures, .. } = &mut instruction.kind else {
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
        let local = locals
            .get_mut(parameter)
            .ok_or_else(|| shape(format!("missing lifted captured closure parameter {parameter:?}")))?;
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

'''
s = s[:idx] + new_code + s[idx:]
write(p, s)

# Run captured lifting after capture-free function-value lifting.
p = "crates/forge-codegen-cranelift/src/backend_c11d_legacy.rs"
s = read(p)
s = replace_once(s,
'''        crate::completion::lift_capture_free_function_values(\n            &mut all_functions,\n            &mut used,\n            &mut next_internal,\n        )?;''',
'''        crate::completion::lift_capture_free_function_values(\n            &mut all_functions,\n            &mut used,\n            &mut next_internal,\n        )?;\n        crate::completion::lift_captured_closure_values(\n            &mut all_functions,\n            &mut used,\n            &mut next_internal,\n        )?;''', p)
# Global closure storage would outlive stack-backed environments.
s = replace_once(s,
'''        let diagnostics = verify_fir_module(module);\n        if !diagnostics.is_empty() {''',
'''        let diagnostics = verify_fir_module(module);\n        if !diagnostics.is_empty() {''', p)
anchor = '''        let mut used = BTreeSet::new();\n'''
replacement = '''        if module\n            .globals\n            .values()\n            .any(|global| matches!(global.ty, Ty::Closure { .. }))\n        {\n            return Err(BackendError::UnsupportedFir {\n                component: "global captured closure storage requires heap/lifetime support",\n            });\n        }\n\n        let mut used = BTreeSet::new();\n'''
s = replace_once(s, anchor, replacement, p)
write(p, s)

# Replace local tag dispatch with the real environment-pointer/trampoline ABI.
p = "crates/forge-codegen-cranelift/src/function_c14_closure.rs"
s = read(p)
# Remove now-obsolete tag from environment metadata.
s = replace_once(s,
'''struct C14ClosureEnvironment {\n    slot: StackSlot,\n    tag: u64,\n    fields: Vec<C14ClosureFieldLayout>,\n}\n\nstruct C14ClosureCandidate {\n    closure: ExprId,\n    setup: Block,\n    blocks: BTreeMap<FirBlockId, Block>,\n}\n\n#[derive(Clone)]\nstruct C14ClosureResult {\n    id: FirValueId,\n    ty: Ty,\n    slot: StackSlot,\n}\n''',
'''struct C14ClosureEnvironment {\n    slot: StackSlot,\n    fields: Vec<C14ClosureFieldLayout>,\n}\n''', p)
# Detect synthetic lifted closure functions before ordinary C14 lowering.
needle = '''pub(crate) fn lower_function_c14(\n    fir: &FirFunction,\n    all_functions: &BTreeMap<DefId, FirFunction>,\n    all_globals: &BTreeMap<DefId, FirGlobal>,\n    definitions: &TypeDefinitionTable,\n    types: &TypeLowering<'_>,\n    isa: &dyn TargetIsa,\n) -> Result<Function, BackendError> {\n'''
replacement = needle + '''    if let Some(closure_id) = fir\n        .blocks\n        .iter()\n        .find(|block| block.id == fir.entry)\n        .and_then(|block| block.closure)\n    {\n        return lower_c14_lifted_closure(\n            fir,\n            closure_id,\n            all_functions,\n            all_globals,\n            definitions,\n            types,\n            isa,\n        );\n    }\n\n'''
s = replace_once(s, needle, replacement, p)
# Insert lifted-function lowering before local-slot allocation.
anchor = '''fn c14_allocate_local_slots(\n'''
idx = s.index(anchor)
lifted = r'''#[allow(clippy::too_many_arguments)]
fn lower_c14_lifted_closure(
    fir: &FirFunction,
    closure_id: ExprId,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    let closure = fir
        .closures
        .get(&closure_id)
        .ok_or_else(|| shape("lifted closure metadata is missing"))?;
    if fir.params.len() != closure.params.len() + 1 {
        return Err(shape("lifted closure hidden environment parameter is missing"));
    }

    let call_conv = CallConv::triple_default(isa.triple());
    let signature_plan = lower_c9_fir_signature(fir, definitions, types, call_conv)?;
    let entry_types = signature_plan
        .signature
        .params
        .iter()
        .map(|param| param.value_type)
        .collect::<Vec<_>>();
    let mut function = Function::with_name_signature(
        UserFuncName::user(0, fir.owner.0),
        signature_plan.signature.clone(),
    );
    let mut layouts = LayoutEngine::new(LayoutTarget::new(types.target().pointer_bits), definitions);
    let local_slots = c14_allocate_local_slots(fir, &mut layouts, types, &mut function)?;
    let environments = c14_allocate_closure_environments(fir, &mut layouts, types, &mut function)?;
    let flags = MemoryFlags {
        stack: MemFlagsData::trusted(),
        deref: MemFlagsData::new(),
    };

    let mut blocks = BTreeMap::new();
    for fir_block in fir.blocks.iter().filter(|block| block.closure == Some(closure_id)) {
        let block = function.dfg.make_block();
        function.layout.append_block(block);
        if blocks.insert(fir_block.id, block).is_some() {
            return Err(shape(format!("duplicate lifted closure FIR block {:?}", fir_block.id)));
        }
    }
    let entry = *blocks
        .get(&fir.entry)
        .ok_or_else(|| shape(format!("missing lifted closure entry block {:?}", fir.entry)))?;
    let entry_values = entry_types
        .into_iter()
        .map(|ty| function.dfg.append_block_param(entry, ty))
        .collect::<Vec<_>>();

    let mut scalars = BTreeMap::<FirValueId, Value>::new();
    let mut aggregates = BTreeMap::<FirValueId, AggregateValue>::new();
    let mut direct_functions = BTreeMap::<DefId, FuncRef>::new();
    let hidden_return = initialize_c9d_parameters(
        fir,
        &signature_plan,
        &entry_values,
        &local_slots,
        flags,
        types,
        &mut layouts,
        &mut function,
    )?;

    let env_local = *fir
        .params
        .first()
        .ok_or_else(|| shape("lifted closure has no hidden environment local"))?;
    let env_slot = *local_slots
        .get(&env_local)
        .ok_or_else(|| shape("lifted closure environment local has no stack slot"))?;
    let environment = {
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(entry);
        let address = cursor.ins().stack_addr(types.pointer_type()?, env_slot, 0);
        cursor
            .ins()
            .load(types.pointer_type()?, flags.stack, address, 0)
    };

    for fir_block in fir.blocks.iter().filter(|block| block.closure == Some(closure_id)) {
        let clif_block = *blocks
            .get(&fir_block.id)
            .ok_or_else(|| shape(format!("missing lifted closure CLIF block for {:?}", fir_block.id)))?;
        let mut cursor = FuncCursor::new(&mut function);
        cursor.goto_bottom(clif_block);
        for instruction in &fir_block.instructions {
            lower_c14_instruction(
                fir,
                all_functions,
                all_globals,
                definitions,
                instruction,
                &local_slots,
                &environments,
                Some((closure_id, environment)),
                flags,
                call_conv,
                &mut direct_functions,
                &mut scalars,
                &mut aggregates,
                types,
                &mut layouts,
                &mut cursor,
            )?;
        }
        let terminator = fir_block
            .terminator
            .as_ref()
            .ok_or_else(|| shape(format!("lifted closure block {:?} has no terminator", fir_block.id)))?;
        lower_c9d_terminator(
            fir,
            terminator,
            &signature_plan.result,
            hidden_return,
            &blocks,
            flags,
            &scalars,
            &aggregates,
            types,
            &mut layouts,
            &mut cursor,
        )?;
    }

    verify_function(&function, isa).map_err(|errors| BackendError::Cranelift {
        message: format!("CLIF verifier rejected lifted C14 closure {:?}: {errors}", fir.owner),
    })?;
    Ok(function)
}

'''
s = s[:idx] + lifted + s[idx:]
# Environment layout no longer assigns local tags.
s = s.replace('''    for (tag_index, (id, closure)) in fir.closures.iter().enumerate() {''', '''    for (id, closure) in &fir.closures {''', 1)
s = replace_once(s,
'''            C14ClosureEnvironment {\n                slot,\n                tag: u64::try_from(tag_index + 1).map_err(|_| shape("too many local closures"))?,\n                fields,\n            },''',
'''            C14ClosureEnvironment { slot, fields },''', p)
# Call site passes only inputs needed by indirect ABI lowering.
old_call = '''            return lower_c14_call_closure(\n                fir,\n                all_functions,\n                all_globals,\n                definitions,\n                instruction,\n                *closure,\n                args,\n                local_slots,\n                environments,\n                flags,\n                call_conv,\n                direct_functions,\n                scalars,\n                aggregates,\n                types,\n                layouts,\n                cursor,\n            );'''
new_call = '''            return lower_c14_call_closure(\n                fir,\n                definitions,\n                instruction,\n                *closure,\n                args,\n                flags,\n                call_conv,\n                scalars,\n                aggregates,\n                types,\n                layouts,\n                cursor,\n            );'''
s = replace_once(s, old_call, new_call, p)
# MakeClosure stores synthetic function address in word 0; remaining inputs are captures.
start = s.index('fn lower_c14_make_closure(')
call_start = s.index('#[allow(clippy::too_many_arguments)]\nfn lower_c14_call_closure(', start)
make = s[start:call_start]
make = replace_once(make,
'''    if captures.len() != environment.fields.len() {\n        return Err(shape("closure capture count differs from environment layout"));\n    }''',
'''    if captures.len() != environment.fields.len() + 1 {\n        return Err(shape("closure code/capture count differs from environment layout"));\n    }''', p)
make = replace_once(make,
'''    let tag = cursor\n        .ins()\n        .iconst(types.pointer_type()?, environment.tag as i64);\n    cursor.ins().store(flags.stack, tag, base, 0);\n\n    for (capture, field) in captures.iter().zip(&environment.fields) {''',
'''    let code = captures[0];\n    if !matches!(value_type(fir, code)?, Ty::Function { .. }) {\n        return Err(shape("captured closure code value is not function typed"));\n    }\n    cursor\n        .ins()\n        .store(flags.stack, scalar(scalars, code)?, base, 0);\n\n    for (capture, field) in captures[1..].iter().zip(&environment.fields) {''', p)
s = s[:start] + make + s[call_start:]
# Replace the old local tag-dispatch implementation and obsolete closure terminator.
call_start = s.index('#[allow(clippy::too_many_arguments)]\nfn lower_c14_call_closure(')
capture_start = s.index('#[allow(clippy::too_many_arguments)]\nfn lower_c14_capture_instruction(', call_start)
new_call_impl = r'''#[allow(clippy::too_many_arguments)]
fn lower_c14_call_closure(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    instruction: &FirInstruction,
    callee: FirValueId,
    args: &[FirValueId],
    flags: MemoryFlags,
    call_conv: CallConv,
    scalars: &mut BTreeMap<FirValueId, Value>,
    aggregates: &mut BTreeMap<FirValueId, AggregateValue>,
    types: &TypeLowering<'_>,
    layouts: &mut LayoutEngine<'_>,
    cursor: &mut FuncCursor<'_>,
) -> Result<(), BackendError> {
    let callee_ty = value_type(fir, callee)?;
    let Ty::Closure {
        params,
        result: result_ty,
    } = callee_ty
    else {
        return Err(shape("call-closure callee is not closure typed"));
    };
    if args.len() != params.len() {
        return Err(shape("closure call argument count mismatch"));
    }
    for (index, (arg, expected)) in args.iter().zip(params).enumerate() {
        let actual = value_type(fir, *arg)?;
        if actual != expected {
            return Err(shape(format!(
                "closure call argument {index} has type {actual:?}, expected {expected:?}"
            )));
        }
    }

    let mut code_params = Vec::with_capacity(params.len() + 1);
    code_params.push(callee_ty.clone());
    code_params.extend(params.iter().cloned());
    let code_ty = Ty::Function {
        params: code_params,
        result: result_ty.clone(),
        named_arguments: false,
    };
    let plan = lower_c9_function_type_signature(&code_ty, definitions, types, call_conv)?;
    let mut call_args = Vec::with_capacity(args.len() + 1);
    call_args.push(callee);
    call_args.extend(args.iter().copied());
    let (lowered_args, indirect_result) = lower_c9d_call_arguments(
        fir,
        &call_args,
        &plan,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )?;

    let environment = scalar(scalars, callee)?;
    let code = cursor
        .ins()
        .load(types.pointer_type()?, flags.deref, environment, 0);
    let sig_ref = cursor.func.import_signature(plan.signature.clone());
    let inst = cursor.ins().call_indirect(sig_ref, code, &lowered_args);
    record_c9d_call_result(
        fir,
        instruction,
        &plan.result,
        indirect_result,
        inst,
        scalars,
        aggregates,
        flags,
        types,
        layouts,
        cursor,
    )
}

'''
s = s[:call_start] + new_call_impl + s[capture_start:]
# Lifted functions access caller-owned environments conservatively as dereferenced memory.
s = replace_once(s,
'''                CaptureMode::Value => Ok((field_address, field.ty.clone(), flags.stack)),\n                CaptureMode::SharedReference | CaptureMode::MutableReference => {\n                    let address = cursor\n                        .ins()\n                        .load(types.pointer_type()?, flags.stack, field_address, 0);''',
'''                CaptureMode::Value => Ok((field_address, field.ty.clone(), flags.deref)),\n                CaptureMode::SharedReference | CaptureMode::MutableReference => {\n                    let address = cursor\n                        .ins()\n                        .load(types.pointer_type()?, flags.deref, field_address, 0);''', p)
write(p, s)

# Executable cross-function coverage.
write("examples/c14-native-spec/run/captured_closure_abi.fg", r'''module examples.c14_native_spec.captured_closure_abi;

fn apply(f: closure(i32) -> i32, value: i32) -> i32 {
    return f(value);
}

fn relay(f: closure(i32) -> i32, value: i32) -> i32 {
    return apply(f, value);
}

fn main() -> i32 {
    val factor: i32 = 3i32;
    val bias: i32 = 1i32;
    val affine = [factor, bias](value: i32) -> i32 {
        return value * factor + bias;
    };
    if (relay(affine, 13i32) != 40i32) {
        return 1;
    }

    var count: i32 = 10i32;
    val bump = [&mut count](delta: i32) -> i32 {
        count = count + delta;
        return count;
    };
    if (apply(bump, 5i32) != 15i32 || count != 15i32) {
        return 2;
    }

    val base_a: i32 = 40i32;
    val base_b: i32 = 100i32;
    val add_a = [base_a](value: i32) -> i32 {
        return base_a + value;
    };
    val add_b = [base_b](value: i32) -> i32 {
        return base_b + value;
    };
    if (apply(add_a, 2i32) != 42i32 || apply(add_b, 2i32) != 102i32) {
        return 3;
    }

    return 0;
}
''')

# Focused target-width layout/ABI proof.
write("crates/forge-fir/tests/closure_abi.rs", r'''use forge_fir::{AbiDecomposer, AbiPieceKind, AbiTarget, LayoutEngine, LayoutTarget, Ty, TypeDefinitionTable};

fn closure_ty() -> Ty {
    Ty::Closure {
        params: vec![Ty::Int {
            signed: true,
            width: forge_fir::IntWidth::W32,
        }],
        result: Box::new(Ty::Int {
            signed: true,
            width: forge_fir::IntWidth::W32,
        }),
    }
}

#[test]
fn closure_layout_is_one_target_pointer() {
    let definitions = TypeDefinitionTable::new();
    for (bits, bytes) in [(32, 4), (64, 8)] {
        let mut layouts = LayoutEngine::new(LayoutTarget::new(bits), &definitions);
        let layout = layouts.layout_of(&closure_ty()).expect("closure layout");
        assert_eq!(layout.size, bytes);
        assert_eq!(layout.align, bytes);
    }
}

#[test]
fn closure_abi_is_one_pointer_piece_on_sia32_and_native64() {
    let definitions = TypeDefinitionTable::new();
    for target in [AbiTarget::sia32(), AbiTarget::native64()] {
        let mut abi = AbiDecomposer::new(target, &definitions).expect("ABI decomposer");
        let decomposition = abi.decompose(&closure_ty()).expect("closure ABI");
        assert_eq!(decomposition.pieces.len(), 1);
        assert_eq!(decomposition.pieces[0].kind, AbiPieceKind::Pointer);
        assert_eq!(decomposition.pieces[0].bits, target.pointer_bits);
    }
}
''')

print("materialized captured closure ABI")
