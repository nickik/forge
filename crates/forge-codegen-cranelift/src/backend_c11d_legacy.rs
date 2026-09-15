include!("backend.rs");

pub(crate) struct C11dPreparedFunctions {
    pub(crate) module: PreparedModule,
    pub(crate) initializer_functions: BTreeMap<DefId, DefId>,
    pub(crate) module_initializer_owner: Option<DefId>,
}

impl CraneliftBackend {
    /// C11d lowers ordinary functions and each `FirGlobalInitializer::function`
    /// through the same verified C9/C11 pipeline. Runtime initializer bodies
    /// receive deterministic internal DefIds so they remain ordinary object
    /// functions without colliding with source definitions.
    pub(crate) fn prepare_functions_and_initializers_with_globals(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<C11dPreparedFunctions, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        let mut used = BTreeSet::new();
        used.extend(module.functions.keys().copied());
        used.extend(module.globals.keys().copied());
        let mut next_internal = u32::MAX;

        let mut all_functions = module.functions.clone();
        let mut initializer_functions = BTreeMap::new();
        for (global_owner, initializer) in &module.global_initializers {
            let global = module.globals.get(global_owner).ok_or_else(|| {
                shape(format!(
                    "runtime initializer refers to missing global {global_owner:?}"
                ))
            })?;
            if initializer.function.return_type != global.ty {
                return Err(shape(format!(
                    "runtime initializer for {global_owner:?} returns {:?}, global type is {:?}",
                    initializer.function.return_type, global.ty
                )));
            }
            if !initializer.function.params.is_empty() {
                return Err(shape(format!(
                    "runtime initializer for {global_owner:?} must not take parameters"
                )));
            }

            let owner = allocate_internal_owner(&mut used, &mut next_internal)?;
            let mut function = initializer.function.clone();
            function.owner = owner;
            if all_functions.insert(owner, function).is_some() {
                return Err(shape(format!(
                    "duplicate synthetic runtime initializer owner {owner:?}"
                )));
            }
            initializer_functions.insert(*global_owner, owner);
        }

        let module_initializer_owner = if module.global_init_order.is_empty() {
            None
        } else {
            Some(allocate_internal_owner(&mut used, &mut next_internal)?)
        };

        let lowering = self.type_lowering();
        let mut functions = BTreeMap::new();
        for (owner, fir) in &all_functions {
            validate_c4_scalar_contract(fir, &self.layout)?;
            validate_c9_memory_places(fir)?;
            let scheduled = schedule_value_blocks(fir)?;
            let function = crate::function::lower_function_with_globals(
                &scheduled,
                &all_functions,
                &module.globals,
                definitions,
                &lowering,
                &*self.isa,
            )?;
            functions.insert(*owner, function);
        }

        if let Some(owner) = module_initializer_owner {
            let function = crate::function::lower_module_initializer(
                owner,
                &module.global_init_order,
                &initializer_functions,
                &all_functions,
                &module.globals,
                definitions,
                &lowering,
                &*self.isa,
            )?;
            functions.insert(owner, function);
        }

        Ok(C11dPreparedFunctions {
            module: PreparedModule {
                target: self.target,
                functions,
            },
            initializer_functions,
            module_initializer_owner,
        })
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
            return Err(shape("no DefId remains for C11d internal functions"));
        }
        *next -= 1;
    }
}
