/// Final C14e normalization layer for source-level types that intentionally do
/// not have an ordinary machine value representation.
///
/// `never` is semantically distinct from `void`, but both have zero ABI return
/// pieces. Keep the public FIR unchanged and construct a private codegen view
/// in which a non-returning result has no machine value. Any block that
/// produces `never` is terminated immediately afterwards so a backend cannot
/// accidentally execute a continuation if a supposedly non-returning callee
/// violates its contract.
pub(crate) fn lower_function_c14_complete(
    fir: &FirFunction,
    all_functions: &BTreeMap<DefId, FirFunction>,
    all_globals: &BTreeMap<DefId, FirGlobal>,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    isa: &dyn TargetIsa,
) -> Result<Function, BackendError> {
    let needs_view = c14_needs_never_codegen_view(fir)
        || all_functions.values().any(c14_needs_never_codegen_view);
    if !needs_view {
        return lower_function_c14_scalar(
            fir,
            all_functions,
            all_globals,
            definitions,
            types,
            isa,
        );
    }

    let transformed_functions = all_functions
        .iter()
        .map(|(owner, function)| (*owner, c14_never_codegen_view(function)))
        .collect::<BTreeMap<_, _>>();
    let transformed = transformed_functions
        .get(&fir.owner)
        .cloned()
        .unwrap_or_else(|| c14_never_codegen_view(fir));

    lower_function_c14_scalar(
        &transformed,
        &transformed_functions,
        all_globals,
        definitions,
        types,
        isa,
    )
}

fn c14_needs_never_codegen_view(fir: &FirFunction) -> bool {
    fir.return_type == Ty::Never
        || fir.value_types.values().any(c14_ty_contains_callable_never)
        || fir.locals.values().any(|local| c14_ty_contains_callable_never(&local.ty))
        || fir
            .closures
            .values()
            .any(|closure| closure.return_type == Ty::Never)
}

fn c14_ty_contains_callable_never(ty: &Ty) -> bool {
    match ty {
        Ty::Never => true,
        Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
            result.as_ref() == &Ty::Never
                || params.iter().any(c14_ty_contains_callable_never)
                || c14_ty_contains_callable_never(result)
        }
        Ty::Pointer { inner, .. }
        | Ty::Reference { inner, .. }
        | Ty::Optional { inner }
        | Ty::Slice { element: inner, .. }
        | Ty::Array { element: inner, .. } => c14_ty_contains_callable_never(inner),
        Ty::Result { ok, error } => {
            c14_ty_contains_callable_never(ok) || c14_ty_contains_callable_never(error)
        }
        _ => false,
    }
}

fn c14_never_codegen_view(fir: &FirFunction) -> FirFunction {
    let mut lowered = fir.clone();
    lowered.return_type = c14_never_codegen_ty(&lowered.return_type);

    for local in lowered.locals.values_mut() {
        local.ty = c14_never_codegen_ty(&local.ty);
    }
    for ty in lowered.value_types.values_mut() {
        *ty = c14_never_codegen_ty(ty);
    }
    for closure in lowered.closures.values_mut() {
        closure.return_type = c14_never_codegen_ty(&closure.return_type);
        for capture in &mut closure.captures {
            capture.ty = c14_never_codegen_ty(&capture.ty);
        }
    }

    // A value of type `never` has no continuation. Preserve that fact in the
    // codegen-only view even though its machine representation is `void`.
    for (original, block) in fir.blocks.iter().zip(&mut lowered.blocks) {
        let never_instruction = original.instructions.iter().position(|instruction| {
            instruction
                .result
                .and_then(|id| fir.value_types.get(&id))
                .is_some_and(|ty| ty == &Ty::Never)
        });
        if let Some(index) = never_instruction {
            block.instructions.truncate(index + 1);
            block.terminator = Some(FirTerminator::Unreachable);
        }
    }

    lowered
}

fn c14_never_codegen_ty(ty: &Ty) -> Ty {
    match ty {
        Ty::Never => Ty::Void,
        Ty::Function {
            params,
            result,
            named_arguments,
        } => Ty::Function {
            params: params.iter().map(c14_never_codegen_ty).collect(),
            result: Box::new(c14_never_codegen_ty(result)),
            named_arguments: *named_arguments,
        },
        Ty::Closure { params, result } => Ty::Closure {
            params: params.iter().map(c14_never_codegen_ty).collect(),
            result: Box::new(c14_never_codegen_ty(result)),
        },
        Ty::Pointer { volatile, inner } => Ty::Pointer {
            volatile: *volatile,
            inner: Box::new(c14_never_codegen_ty(inner)),
        },
        Ty::Reference { mutable, inner } => Ty::Reference {
            mutable: *mutable,
            inner: Box::new(c14_never_codegen_ty(inner)),
        },
        Ty::Optional { inner } => Ty::Optional {
            inner: Box::new(c14_never_codegen_ty(inner)),
        },
        Ty::Slice { mutable, element } => Ty::Slice {
            mutable: *mutable,
            element: Box::new(c14_never_codegen_ty(element)),
        },
        Ty::Array { element, length } => Ty::Array {
            element: Box::new(c14_never_codegen_ty(element)),
            length: *length,
        },
        Ty::Result { ok, error } => Ty::Result {
            ok: Box::new(c14_never_codegen_ty(ok)),
            error: Box::new(c14_never_codegen_ty(error)),
        },
        other => other.clone(),
    }
}
