include!("backend.rs");

impl CraneliftBackend {
    /// C11c function preparation keeps the full FIR global table available to
    /// lowering while retaining the proven C10/C9 validation and scheduling
    /// pipeline for ordinary functions.
    pub(crate) fn prepare_functions_with_globals(
        &self,
        module: &FirModule,
        definitions: &TypeDefinitionTable,
    ) -> Result<PreparedModule, BackendError> {
        let diagnostics = verify_fir_module(module);
        if !diagnostics.is_empty() {
            return Err(BackendError::InvalidFir {
                diagnostic_count: diagnostics.len(),
            });
        }

        let lowering = self.type_lowering();
        let mut functions = BTreeMap::new();
        for (owner, fir) in &module.functions {
            validate_c4_scalar_contract(fir, &self.layout)?;
            validate_c9_memory_places(fir)?;
            let scheduled = schedule_value_blocks(fir)?;
            let function = crate::function::lower_function_with_globals(
                &scheduled,
                &module.functions,
                &module.globals,
                definitions,
                &lowering,
                &*self.isa,
            )?;
            functions.insert(*owner, function);
        }

        Ok(PreparedModule {
            target: self.target,
            functions,
        })
    }
}
