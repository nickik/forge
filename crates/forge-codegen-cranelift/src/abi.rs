use cranelift_codegen::ir::{types as clif_types, AbiParam, Signature};
use cranelift_codegen::isa::CallConv;
use forge_fir::{
    AbiDecomposer, AbiDecomposition, AbiError, AbiPassing, AbiPiece, AbiPieceKind, AbiTarget,
    FirFunction, Ty, TypeDefinitionTable,
};

use crate::{BackendError, TypeLowering};

/// C8 scalar signature lowering retained for the C9c compatibility path.
pub(crate) fn lower_fir_signature(
    fir: &FirFunction,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<Signature, BackendError> {
    let params = fir_parameter_types(fir)?;
    lower_signature(&params, &fir.return_type, types, call_conv)
}

/// C8 scalar first-class function signature lowering retained for C9c.
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
        signature.params.push(AbiParam::new(types.value_type(ty)?));
    }
    if *result != Ty::Void {
        signature
            .returns
            .push(AbiParam::new(types.value_type(result)?));
    }
    Ok(signature)
}

#[derive(Clone, Debug)]
pub(crate) enum C9ParamPlan {
    Scalar {
        ty: Ty,
    },
    AggregateDirect {
        ty: Ty,
        decomposition: AbiDecomposition,
    },
    AggregateIndirect {
        ty: Ty,
        decomposition: AbiDecomposition,
    },
}

impl C9ParamPlan {
    pub(crate) fn ty(&self) -> &Ty {
        match self {
            Self::Scalar { ty }
            | Self::AggregateDirect { ty, .. }
            | Self::AggregateIndirect { ty, .. } => ty,
        }
    }

    pub(crate) fn clif_param_count(&self) -> usize {
        match self {
            Self::Scalar { .. } | Self::AggregateIndirect { .. } => 1,
            Self::AggregateDirect { decomposition, .. } => decomposition.pieces.len(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum C9ReturnPlan {
    Void,
    Scalar {
        ty: Ty,
    },
    AggregateDirect {
        ty: Ty,
        decomposition: AbiDecomposition,
    },
    AggregateIndirect {
        ty: Ty,
        decomposition: AbiDecomposition,
    },
}

impl C9ReturnPlan {
    pub(crate) fn ty(&self) -> Option<&Ty> {
        match self {
            Self::Void => None,
            Self::Scalar { ty }
            | Self::AggregateDirect { ty, .. }
            | Self::AggregateIndirect { ty, .. } => Some(ty),
        }
    }

    pub(crate) fn is_indirect(&self) -> bool {
        matches!(self, Self::AggregateIndirect { .. })
    }
}

#[derive(Clone, Debug)]
pub(crate) struct C9SignaturePlan {
    pub(crate) signature: Signature,
    pub(crate) params: Vec<C9ParamPlan>,
    pub(crate) result: C9ReturnPlan,
}

pub(crate) fn is_c9_aggregate_type(ty: &Ty) -> bool {
    matches!(
        ty,
        Ty::Str
            | Ty::Nominal(_)
            | Ty::Optional { .. }
            | Ty::Slice { .. }
            | Ty::Array { .. }
            | Ty::Result { .. }
    )
}

pub(crate) fn lower_c9_fir_signature(
    fir: &FirFunction,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<C9SignaturePlan, BackendError> {
    lower_c9_signature(
        &fir_parameter_types(fir)?,
        &fir.return_type,
        definitions,
        types,
        call_conv,
    )
}

pub(crate) fn lower_c9_function_type_signature(
    ty: &Ty,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<C9SignaturePlan, BackendError> {
    let Ty::Function {
        params,
        result,
        named_arguments: _,
    } = ty
    else {
        return Err(shape(format!(
            "C9 indirect call callee has non-function FIR type {ty:?}"
        )));
    };
    lower_c9_signature(params, result, definitions, types, call_conv)
}

fn lower_c9_signature(
    params: &[Ty],
    result: &Ty,
    definitions: &TypeDefinitionTable,
    types: &TypeLowering<'_>,
    call_conv: CallConv,
) -> Result<C9SignaturePlan, BackendError> {
    let target = c9_abi_target(types)?;
    let mut decomposer = AbiDecomposer::new(target, definitions).map_err(abi_error)?;
    let result_plan = lower_c9_return(result, &mut decomposer)?;
    let mut signature = Signature::new(call_conv);

    // Indirect aggregate return storage is the first hidden Forge ABI argument.
    // This is an ordinary pointer parameter, not Cranelift's C StructReturn ABI.
    if result_plan.is_indirect() {
        signature.params.push(AbiParam::new(types.pointer_type()?));
    }

    let mut param_plans = Vec::with_capacity(params.len());
    for ty in params {
        if *ty == Ty::Void {
            return Err(shape("void FIR parameter reached C9 ABI lowering"));
        }
        let plan = if is_c9_aggregate_type(ty) {
            let decomposition = decomposer.decompose(ty).map_err(abi_error)?;
            match decomposition.passing {
                AbiPassing::Direct => {
                    for piece in &decomposition.pieces {
                        signature
                            .params
                            .push(AbiParam::new(c9_piece_type(piece, types)?));
                    }
                    C9ParamPlan::AggregateDirect {
                        ty: ty.clone(),
                        decomposition,
                    }
                }
                AbiPassing::Indirect => {
                    signature.params.push(AbiParam::new(types.pointer_type()?));
                    C9ParamPlan::AggregateIndirect {
                        ty: ty.clone(),
                        decomposition,
                    }
                }
            }
        } else {
            signature.params.push(AbiParam::new(types.value_type(ty)?));
            C9ParamPlan::Scalar { ty: ty.clone() }
        };
        param_plans.push(plan);
    }

    match &result_plan {
        C9ReturnPlan::Void | C9ReturnPlan::AggregateIndirect { .. } => {}
        C9ReturnPlan::Scalar { ty } => {
            signature.returns.push(AbiParam::new(types.value_type(ty)?));
        }
        C9ReturnPlan::AggregateDirect { decomposition, .. } => {
            for piece in &decomposition.pieces {
                signature
                    .returns
                    .push(AbiParam::new(c9_piece_type(piece, types)?));
            }
        }
    }

    Ok(C9SignaturePlan {
        signature,
        params: param_plans,
        result: result_plan,
    })
}

fn lower_c9_return(
    ty: &Ty,
    decomposer: &mut AbiDecomposer<'_>,
) -> Result<C9ReturnPlan, BackendError> {
    if *ty == Ty::Void {
        return Ok(C9ReturnPlan::Void);
    }
    if !is_c9_aggregate_type(ty) {
        return Ok(C9ReturnPlan::Scalar { ty: ty.clone() });
    }
    let decomposition = decomposer.decompose(ty).map_err(abi_error)?;
    Ok(match decomposition.passing {
        AbiPassing::Direct => C9ReturnPlan::AggregateDirect {
            ty: ty.clone(),
            decomposition,
        },
        AbiPassing::Indirect => C9ReturnPlan::AggregateIndirect {
            ty: ty.clone(),
            decomposition,
        },
    })
}

pub(crate) fn c9_piece_type(
    piece: &AbiPiece,
    types: &TypeLowering<'_>,
) -> Result<cranelift_codegen::ir::Type, BackendError> {
    match piece.kind {
        AbiPieceKind::Pointer => {
            if piece.bits != types.target().pointer_bits {
                return Err(shape(format!(
                    "Forge ABI pointer piece is {} bits on a {}-bit target",
                    piece.bits,
                    types.target().pointer_bits
                )));
            }
            types.pointer_type()
        }
        AbiPieceKind::Integer => match piece.bits {
            8 => Ok(clif_types::I8),
            16 => Ok(clif_types::I16),
            32 => Ok(clif_types::I32),
            64 => Ok(clif_types::I64),
            bits => Err(shape(format!(
                "unsupported Forge ABI integer piece width {bits}"
            ))),
        },
    }
}

fn c9_abi_target(types: &TypeLowering<'_>) -> Result<AbiTarget, BackendError> {
    match types.target().pointer_bits {
        32 => Ok(AbiTarget::sia32()),
        64 => Ok(AbiTarget::native64()),
        pointer_bits => Err(BackendError::UnsupportedTargetLayout { pointer_bits }),
    }
}

fn abi_error(error: AbiError) -> BackendError {
    BackendError::InvalidFirShape {
        message: format!("Forge ABI decomposition failed during C9 lowering: {error:?}"),
    }
}

fn shape(message: impl Into<String>) -> BackendError {
    BackendError::InvalidFirShape {
        message: message.into(),
    }
}
