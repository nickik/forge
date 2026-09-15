use cranelift_codegen::ir::{AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
use forge_fir::{FirFunction, Ty};

use crate::{BackendError, TypeLowering};

/// Lower a Forge FIR function signature into the target calling convention's
/// scalar CLIF signature.
///
/// C8 intentionally does not use Cranelift's C-struct ABI facilities. Aggregate
/// decomposition/passing is a Forge ABI decision deferred to C9.
pub(crate) fn lower_fir_signature(
    fir: &FirFunction,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<Signature, BackendError> {
    let mut params = Vec::with_capacity(fir.params.len());
    for local_id in &fir.params {
        let local = fir
            .locals
            .get(local_id)
            .ok_or_else(|| shape(format!("missing FIR parameter local {local_id:?}")))?;
        params.push(local.ty.clone());
    }
    lower_signature(&params, &fir.return_type, types, call_conv)
}

/// Lower a first-class Forge function type into a CLIF call signature.
pub(crate) fn lower_function_type_signature(
    ty: &Ty,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<Signature, BackendError> {
    let Ty::Function {
        params,
        result,
        named_arguments: _,
    } = ty
    else {
        return Err(shape(format!(
            "indirect call callee has non-function FIR type {ty:?}"
        )));
    };
    lower_signature(params, result, types, call_conv)
}

pub(crate) fn fir_parameter_types(fir: &FirFunction) -> Result<Vec<Ty>, BackendError> {
    fir.params
        .iter()
        .map(|local_id| {
            fir.locals
                .get(local_id)
                .map(|local| local.ty.clone())
                .ok_or_else(|| shape(format!("missing FIR parameter local {local_id:?}")))
        })
        .collect()
}

fn lower_signature(
    params: &[Ty],
    result: &Ty,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<Signature, BackendError> {
    let mut signature = Signature::new(call_conv);
    for ty in params {
        if *ty == Ty::Void {
            return Err(shape("void FIR parameter reached ABI lowering"));
        }
        signature
            .params
            .push(AbiParam::new(types.value_type(ty)?));
    }
    if *result != Ty::Void {
        signature
            .returns
            .push(AbiParam::new(types.value_type(result)?));
    }
    Ok(signature)
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
