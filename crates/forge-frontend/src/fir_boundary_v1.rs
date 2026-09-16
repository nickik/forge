use std::collections::BTreeMap;

use crate::{
    ast::{FdnValue, Span},
    body_hir::{BodyHirOutput, HirExpr, HirExprKind},
    fir::{self, FirDiagnostic, FirInstructionKind, FirModule, FirOutput},
    typecheck::{Ty, TypeCheckOutput, TypedBody, TypedExpr, TypedExprKind},
};

fn diagnostic(span: Span, code: &str, message: impl Into<String>) -> FirDiagnostic {
    FirDiagnostic {
        span,
        code: code.into(),
        message: message.into(),
    }
}

fn type_is_concrete(ty: &Ty) -> bool {
    match ty {
        Ty::Error | Ty::Unknown | Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral => false,
        Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } | Ty::Optional { inner } => {
            type_is_concrete(inner)
        }
        Ty::Slice { element, .. } => type_is_concrete(element),
        Ty::Array { element, length } => length.is_some() && type_is_concrete(element),
        Ty::Result { ok, error } => type_is_concrete(ok) && type_is_concrete(error),
        Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
            params.iter().all(type_is_concrete) && type_is_concrete(result)
        }
        _ => true,
    }
}

fn typed_source(kind: &TypedExprKind) -> &HirExpr {
    match kind {
        TypedExprKind::Source { hir }
        | TypedExprKind::ResolvedCall { hir, .. }
        | TypedExprKind::ResolvedClosure { hir, .. }
        | TypedExprKind::ResolvedContext { hir, .. }
        | TypedExprKind::ResolvedTry { hir, .. }
        | TypedExprKind::ResolvedMatch { hir, .. }
        | TypedExprKind::UnsafeOperation { hir, .. }
        | TypedExprKind::ResolvedBitField { hir, .. }
        | TypedExprKind::BuiltinConstructor { hir, .. }
        | TypedExprKind::OptionalPromote { hir, .. } => hir,
    }
}

fn verify_expr_kind(
    expression: &TypedExpr,
    kind: &TypedExprKind,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    let hir = typed_source(kind);
    match kind {
        TypedExprKind::Source { .. } => match &hir.kind {
            HirExprKind::Context { .. }
            | HirExprKind::Try { .. }
            | HirExprKind::Match { .. }
            | HirExprKind::Closure { .. } => diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-source",
                format!(
                    "source-only semantic expression reached the FIR boundary: {:?}",
                    hir.kind
                ),
            )),
            HirExprKind::ReaderForm { tag, value }
                if expression.ty == Ty::Duration
                    && tag == "duration"
                    && matches!(value, FdnValue::String { .. }) => {}
            HirExprKind::ReaderForm { .. } => diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-source",
                "unresolved reader form reached the FIR boundary",
            )),
            _ => {}
        },
        TypedExprKind::ResolvedCall { .. } if !matches!(&hir.kind, HirExprKind::Call { .. }) => {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved call is not backed by call HIR",
            ));
        }
        TypedExprKind::ResolvedClosure { .. }
            if !matches!(&hir.kind, HirExprKind::Closure { .. }) =>
        {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved closure is not backed by closure HIR",
            ));
        }
        TypedExprKind::ResolvedContext { .. }
            if !matches!(&hir.kind, HirExprKind::Context { .. }) =>
        {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved context slot is not backed by context HIR",
            ));
        }
        TypedExprKind::ResolvedTry { .. } if !matches!(&hir.kind, HirExprKind::Try { .. }) => {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved try is not backed by try HIR",
            ));
        }
        TypedExprKind::ResolvedMatch { .. } if !matches!(&hir.kind, HirExprKind::Match { .. }) => {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved match is not backed by match HIR",
            ));
        }
        TypedExprKind::ResolvedBitField { .. }
            if !matches!(&hir.kind, HirExprKind::Member { .. }) =>
        {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "resolved bit-field access is not backed by member HIR",
            ));
        }
        TypedExprKind::UnsafeOperation { .. }
            if !matches!(
                &hir.kind,
                HirExprKind::Unary { .. }
                    | HirExprKind::Binary { .. }
                    | HirExprKind::TypeCall { .. }
            ) =>
        {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-shape",
                "unsafe operation has an invalid HIR shape",
            ));
        }
        TypedExprKind::OptionalPromote { inner, .. } => {
            verify_expr_kind(expression, inner, diagnostics);
        }
        _ => {}
    }
}

fn verify_typed_body(body: &TypedBody, diagnostics: &mut Vec<FirDiagnostic>) {
    if !type_is_concrete(&body.return_type) {
        diagnostics.push(diagnostic(
            Span::new(0, 0),
            "fir/boundary-type",
            format!(
                "typed body {:?} has non-concrete return type {:?}",
                body.owner, body.return_type
            ),
        ));
    }
    for (local, ty) in &body.params {
        if !type_is_concrete(ty) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("typed parameter {local:?} has non-concrete type {ty:?}"),
            ));
        }
    }
    for (local, ty) in &body.local_types {
        if !type_is_concrete(ty) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("typed local {local:?} has non-concrete type {ty:?}"),
            ));
        }
    }

    for expression in &body.expressions {
        if !type_is_concrete(&expression.ty) {
            diagnostics.push(diagnostic(
                expression.span,
                "fir/boundary-type",
                format!(
                    "typed expression {:?} has non-concrete type {:?}",
                    expression.id, expression.ty
                ),
            ));
        }
        verify_expr_kind(expression, &expression.kind, diagnostics);
    }
}

pub fn verify_fir_boundary(bodies: &BodyHirOutput, typed: &TypeCheckOutput) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();

    for (owner, body) in &bodies.functions {
        let Some(typed_body) = typed.functions.get(owner) else {
            diagnostics.push(diagnostic(
                body.block.span,
                "fir/boundary-body",
                format!("function {owner:?} has no typed body"),
            ));
            continue;
        };
        verify_typed_body(typed_body, &mut diagnostics);
    }

    for (owner, ty) in &typed.global_types {
        if !type_is_concrete(ty) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("global {owner:?} has non-concrete type {ty:?}"),
            ));
        }
    }

    for (owner, initializer) in &typed.global_initializers {
        if initializer.owner != *owner || initializer.body.owner != *owner {
            diagnostics.push(diagnostic(
                initializer.span,
                "fir/boundary-global-init",
                format!("runtime initializer key/owner mismatch for {owner:?}"),
            ));
        }
        if initializer.body.return_type != initializer.ty {
            diagnostics.push(diagnostic(
                initializer.span,
                "fir/boundary-global-init",
                format!("runtime initializer {owner:?} body/result type mismatch"),
            ));
        }
        verify_typed_body(&initializer.body, &mut diagnostics);
    }

    diagnostics
}

fn verify_no_poison(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for instruction in &block.instructions {
            if matches!(&instruction.kind, FirInstructionKind::Poison) {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-poison",
                    format!("FIR function {:?} contains Poison", function.owner),
                ));
            }
            if let Some(result) = instruction.result {
                if let Some(ty) = function.value_types.get(&result) {
                    if !type_is_concrete(ty) {
                        diagnostics.push(diagnostic(
                            instruction.span,
                            "fir/verify-type",
                            format!("value {result:?} has non-concrete type {ty:?}"),
                        ));
                    }
                }
            }
        }
    }
}

pub fn verify_fir_module(module: &FirModule) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();

    for (owner, global) in &module.globals {
        if global.owner != *owner || !type_is_concrete(&global.ty) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global",
                format!("global {owner:?} has invalid owner/type metadata"),
            ));
        }
    }

    let mut positions = BTreeMap::new();
    for (index, owner) in module.global_init_order.iter().copied().enumerate() {
        if positions.insert(owner, index).is_some() {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} appears more than once"),
            ));
        }
    }
    if positions.len() != module.global_initializers.len()
        || module
            .global_initializers
            .keys()
            .any(|owner| !positions.contains_key(owner))
    {
        diagnostics.push(diagnostic(
            Span::new(0, 0),
            "fir/verify-global-init",
            "module initializer order does not contain every runtime initializer exactly once",
        ));
    }

    for (owner, initializer) in &module.global_initializers {
        let Some(global) = module.globals.get(owner) else {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} has no global"),
            ));
            continue;
        };
        if initializer.owner != *owner || initializer.function.return_type != global.ty {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} has invalid owner/result type"),
            ));
        }
        let owner_position = positions.get(owner).copied();
        for dependency in &initializer.dependencies {
            let dependency_position = positions.get(dependency).copied();
            if dependency_position.is_none()
                || owner_position.is_none()
                || dependency_position >= owner_position
            {
                diagnostics.push(diagnostic(
                    Span::new(0, 0),
                    "fir/verify-global-init-order",
                    format!(
                        "runtime initializer dependency {dependency:?} does not precede {owner:?}"
                    ),
                ));
            }
        }
        diagnostics.extend(fir::verify_fir_function(&initializer.function));
        verify_no_poison(&initializer.function, &mut diagnostics);
    }

    for function in module.functions.values() {
        diagnostics.extend(fir::verify_fir_function(function));
        verify_no_poison(function, &mut diagnostics);
    }

    diagnostics
}

pub fn lower_fir(bodies: &BodyHirOutput, typed: &TypeCheckOutput) -> FirOutput {
    let boundary_diagnostics = verify_fir_boundary(bodies, typed);
    let mut output = fir::lower_fir(bodies, typed);
    output.diagnostics.splice(0..0, boundary_diagnostics);
    output.diagnostics.extend(verify_fir_module(&output.module));
    output
}

pub fn dump_fir_module(module: &FirModule) -> String {
    serde_json::to_string_pretty(module).expect("FIR module serialization")
}
