/// C11d synthesizes one void module initializer. Each runtime initializer is
/// called exactly once in `global_init_order`; its C9 return representation is
/// written directly into the corresponding C11b global storage.
pub(crate) fn lower_module_initializer_c11d(
    owner: DefId,
    init_order: &[DefId],
    initializer_functions: &BTreeMap<DefId, DefId>,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    let call_conv = CallConv::triple_default(isa.triple());
    let mut function = Function::with_name_signature(
        UserFuncName::user(0, owner.0),
        cranelift_codegen::ir::Signature::new(call_conv),
    );
    let entry = function.dfg.make_block();
    function.layout.append_block(entry);
    let mut cursor = FuncCursor::new(&mut function);
    cursor.goto_bottom(entry);
    let mut direct_functions = BTreeMap::<DefId, FuncRef>::new();

    for global_owner in init_order {
        let initializer_owner = initializer_functions.get(global_owner).copied().ok_or_else(|| {
            shape(format!(
                "global initializer order refers to {global_owner:?} without a compiled initializer"
            ))
        })?;
        let initializer = all_functions.get(&initializer_owner).ok_or_else(|| {
            shape(format!(
                "compiled runtime initializer {initializer_owner:?} is missing"
            ))
        })?;
        let global = all_globals.get(global_owner).ok_or_else(|| {
            shape(format!(
                "module initializer refers to missing global {global_owner:?}"
            ))
        })?;
        if !initializer.params.is_empty() {
            return Err(shape(format!(
                "runtime initializer {initializer_owner:?} unexpectedly has parameters"
            )));
        }
        if initializer.return_type != global.ty {
            return Err(shape(format!(
                "runtime initializer {initializer_owner:?} returns {:?}, global {global_owner:?} is {:?}",
                initializer.return_type, global.ty
            )));
        }

        let plan = lower_c9_fir_signature(initializer, definitions, types, call_conv)?;
        if !plan.params.is_empty() {
            return Err(shape(format!(
                "runtime initializer {initializer_owner:?} produced non-empty C9 parameter plan"
            )));
        }
        let func_ref =
            import_c9d_direct_function(initializer_owner, &plan, &mut direct_functions, &mut cursor)?;

        let name = cursor
            .func
            .declare_imported_user_function(UserExternalName::new(1, global_owner.0));
        let symbolic = cursor.func.create_global_value(GlobalValueData::Symbol {
            name: ExternalName::user(name),
            offset: 0.into(),
            colocated: true,
            tls: false,
        });
        let destination = cursor.ins().symbol_value(types.pointer_type()?, symbolic);

        match &plan.result {
            C9ReturnPlan::Void => {
                return Err(shape(format!(
                    "runtime initializer {initializer_owner:?} cannot initialize a global from void"
                )));
            }
            C9ReturnPlan::Scalar { ty } => {
                let inst = cursor.ins().call(func_ref, &[]);
                let results = cursor.func.dfg.inst_results(inst).to_vec();
                let [value] = results.as_slice() else {
                    return Err(shape(format!(
                        "scalar runtime initializer {initializer_owner:?} returned {} CLIF values",
                        results.len()
                    )));
                };
                if cursor.func.dfg.value_type(*value) != types.value_type(ty)? {
                    return Err(shape(format!(
                        "scalar runtime initializer {initializer_owner:?} has wrong CLIF result type"
                    )));
                }
                cursor
                    .ins()
                    .store(MemFlags::new(), *value, destination, 0);
            }
            C9ReturnPlan::AggregateDirect { decomposition, .. } => {
                let inst = cursor.ins().call(func_ref, &[]);
                let results = cursor.func.dfg.inst_results(inst).to_vec();
                if results.len() != decomposition.pieces.len() {
                    return Err(shape(format!(
                        "direct aggregate runtime initializer {initializer_owner:?} returned {} pieces, expected {}",
                        results.len(),
                        decomposition.pieces.len()
                    )));
                }
                unpack_abi_pieces(
                    decomposition,
                    &results,
                    destination,
                    MemFlags::new(),
                    types,
                    &mut cursor,
                )?;
            }
            C9ReturnPlan::AggregateIndirect { .. } => {
                let inst = cursor.ins().call(func_ref, &[destination]);
                if !cursor.func.dfg.inst_results(inst).is_empty() {
                    return Err(shape(format!(
                        "indirect aggregate runtime initializer {initializer_owner:?} returned CLIF values"
                    )));
                }
            }
        }
    }

    cursor.ins().return_(&[]);
    verify_function(&function, isa).map_err(|errors| BackendError::Cranelift {
        message: format!("CLIF verifier rejected C11d module initializer: {errors}"),
    })?;
    Ok(function)
}
