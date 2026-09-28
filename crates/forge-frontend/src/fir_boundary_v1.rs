use std::collections::{BTreeMap, BTreeSet};

use crate::{
    ast::{FdnValue, Span},
    body_hir::{BodyHirOutput, ExprId, HirExpr, HirExprKind},
    fir::{self, FirDiagnostic, FirInstructionKind, FirModule, FirOutput},
    hir::DefId,
    typecheck::{
        CaptureMode, ConstValue, IntWidth, Ty, TypeCheckOutput, TypeDefinitionKind,
        TypeDefinitionTable, TypedBody, TypedExpr, TypedExprKind,
    },
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

fn global_constant_matches_type(constant: &ConstValue, ty: &Ty) -> bool {
    match constant {
        ConstValue::Integer { .. } => matches!(ty, Ty::Byte | Ty::Int { .. }),
        ConstValue::Bool { .. } => matches!(ty, Ty::Bool),
        ConstValue::Char { .. } => matches!(ty, Ty::Char),
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
        | TypedExprKind::Sia32Privileged { hir, .. }
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
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if matches!(&instruction.kind, FirInstructionKind::Poison) {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-poison",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} contains Poison",
                        function.owner, block.id
                    ),
                ));
            }
            if let Some(result) = instruction.result {
                if let Some(ty) = function.value_types.get(&result) {
                    if !type_is_concrete(ty) {
                        diagnostics.push(diagnostic(
                            instruction.span,
                            "fir/verify-type",
                            format!(
                                "FIR function {:?} block {:?} instruction {instruction_index} value {result:?} has non-concrete type {ty:?}",
                                function.owner, block.id
                            ),
                        ));
                    }
                }
            }
        }
    }
}

fn verify_static_data_addresses(
    function: &fir::FirFunction,
    module: &FirModule,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    let expected = Ty::Pointer {
        volatile: false,
        inner: Box::new(Ty::Byte),
    };
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::StaticDataAddress { global } = &instruction.kind else {
                continue;
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let storage = module.globals.get(global);
            let valid_storage = storage.is_some_and(|storage| {
                !storage.mutable
                    && matches!(
                        &storage.ty,
                        Ty::Array {
                            element,
                            length: Some(_),
                        } if element.as_ref() == &Ty::Byte
                    )
            });
            if result_type != Some(&expected) || !valid_storage {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-static-data-address",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} static-data address result {:?} has type {result_type:?} and global {global:?} has metadata {storage:?}; expected a non-volatile byte pointer to immutable fixed-length byte-array storage",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_global_accesses(
    function: &fir::FirFunction,
    module: &FirModule,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (code, valid, facts) = match &instruction.kind {
                FirInstructionKind::LoadGlobal { global } => {
                    let storage = module.globals.get(global);
                    let result_type = instruction
                        .result
                        .and_then(|result| function.value_types.get(&result));
                    (
                        "fir/verify-global-load",
                        storage.is_some_and(|storage| result_type == Some(&storage.ty)),
                        format!(
                            "load result {:?} has type {result_type:?} and global {global:?} has metadata {storage:?}; expected a result with the exact global type",
                            instruction.result
                        ),
                    )
                }
                FirInstructionKind::StoreGlobal { global, value } => {
                    let storage = module.globals.get(global);
                    let value_type = function.value_types.get(value);
                    (
                        "fir/verify-global-store",
                        instruction.result.is_none()
                            && storage.is_some_and(|storage| {
                                storage.mutable && value_type == Some(&storage.ty)
                            }),
                        format!(
                            "store result {:?} uses value {value:?} with type {value_type:?} and global {global:?} has metadata {storage:?}; expected no result, mutable storage, and the exact global value type",
                            instruction.result
                        ),
                    )
                }
                FirInstructionKind::AddressOfGlobal { global, mutable } => {
                    let storage = module.globals.get(global);
                    let result_type = instruction
                        .result
                        .and_then(|result| function.value_types.get(&result));
                    let expected = storage.map(|storage| Ty::Reference {
                        mutable: *mutable,
                        inner: Box::new(storage.ty.clone()),
                    });
                    (
                        "fir/verify-global-address",
                        storage.is_some_and(|storage| {
                            (!*mutable || storage.mutable) && result_type == expected.as_ref()
                        }),
                        format!(
                            "address result {:?} has type {result_type:?}, requested mutable={mutable}, and global {global:?} has metadata {storage:?}; expected result type {expected:?} and compatible storage mutability",
                            instruction.result
                        ),
                    )
                }
                _ => continue,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    code,
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} global access is invalid: {facts}",
                        function.owner, block.id
                    ),
                ));
            }
        }
    }
}

fn unsigned_integer_bits(ty: &Ty) -> Option<u32> {
    match ty {
        Ty::Byte
        | Ty::Int {
            signed: false,
            width: IntWidth::W8,
        } => Some(8),
        Ty::Int {
            signed: false,
            width: IntWidth::W16,
        } => Some(16),
        Ty::Int {
            signed: false,
            width: IntWidth::W32,
        } => Some(32),
        Ty::Int {
            signed: false,
            width: IntWidth::W64,
        } => Some(64),
        _ => None,
    }
}

fn bitfield_value_bits(ty: &Ty) -> Option<u32> {
    if ty == &Ty::Bool {
        Some(1)
    } else {
        unsigned_integer_bits(ty)
    }
}

fn verify_bitfield_width_operations(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (code, valid, facts) = match &instruction.kind {
                FirInstructionKind::BitFieldCheck { value, width } => {
                    let value_type = function.value_types.get(value);
                    let value_bits = value_type.and_then(unsigned_integer_bits);
                    (
                        "fir/verify-bitfield-check",
                        instruction.result.is_none()
                            && value_bits.is_some_and(|bits| *width > 0 && *width < bits),
                        format!(
                            "range check result {:?} uses value {value:?} with type {value_type:?} ({value_bits:?} bits) and field width {width}; expected no result and an unsigned field width strictly between zero and its value width",
                            instruction.result
                        ),
                    )
                }
                FirInstructionKind::BitFieldExtract { value } => {
                    let source_type = function.value_types.get(value);
                    let source_bits = source_type.and_then(unsigned_integer_bits);
                    let result_type = instruction
                        .result
                        .and_then(|result| function.value_types.get(&result));
                    let result_bits = result_type.and_then(bitfield_value_bits);
                    (
                        "fir/verify-bitfield-extract",
                        source_bits.is_some()
                            && result_bits.is_some()
                            && result_bits <= source_bits,
                        format!(
                            "extract value {value:?} has type {source_type:?} ({source_bits:?} bits) and result {:?} has type {result_type:?} ({result_bits:?} bits); expected dedicated unsigned narrowing without widening",
                            instruction.result
                        ),
                    )
                }
                FirInstructionKind::BitFieldExtend { value } => {
                    let source_type = function.value_types.get(value);
                    let source_bits = source_type.and_then(bitfield_value_bits);
                    let result_type = instruction
                        .result
                        .and_then(|result| function.value_types.get(&result));
                    let result_bits = result_type.and_then(unsigned_integer_bits);
                    (
                        "fir/verify-bitfield-extend",
                        source_bits.is_some()
                            && result_bits.is_some()
                            && source_bits <= result_bits,
                        format!(
                            "extend value {value:?} has type {source_type:?} ({source_bits:?} bits) and result {:?} has type {result_type:?} ({result_bits:?} bits); expected a boolean or unsigned field value widened to unsigned storage",
                            instruction.result
                        ),
                    )
                }
                _ => continue,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    code,
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} bitfield operation is invalid: {facts}",
                        function.owner, block.id
                    ),
                ));
            }
        }
    }
}

fn module_function_signature(module: &FirModule, target: DefId) -> Option<(Vec<Ty>, Ty)> {
    module.functions.get(&target).and_then(|callee| {
        callee
            .params
            .iter()
            .map(|parameter| callee.locals.get(parameter).map(|local| local.ty.clone()))
            .collect::<Option<Vec<_>>>()
            .map(|params| (params, callee.return_type.clone()))
    })
}

fn verify_function_references(
    function: &fir::FirFunction,
    module: &FirModule,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::FunctionRef { target } = &instruction.kind else {
                continue;
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let expected = module_function_signature(module, *target);
            let valid = matches!(
                (result_type, expected.as_ref()),
                (
                    Some(Ty::Function { params, result, .. }),
                    Some((expected_params, expected_result)),
                ) if params == expected_params && result.as_ref() == expected_result
            );
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-function-ref",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} function reference result {:?} has type {result_type:?} and target {target:?} has signature {expected:?}; expected an existing module function with an exact function result type",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_calls(
    function: &fir::FirFunction,
    module: &FirModule,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::Call { target, args, .. } = &instruction.kind else {
                continue;
            };
            let argument_types = args
                .iter()
                .map(|argument| function.value_types.get(argument).cloned())
                .collect::<Option<Vec<_>>>();
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let expected = module_function_signature(module, *target);
            let valid = expected.as_ref().is_some_and(|(params, result)| {
                argument_types.as_ref() == Some(params)
                    && match instruction.result {
                        Some(_) => result_type == Some(result),
                        None => result == &Ty::Void,
                    }
            });
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-direct-call",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} direct call to {target:?} uses arguments {args:?} with types {argument_types:?} and result {:?} with type {result_type:?}; target signature is {expected:?}, expected an existing module function with exact argument and result types",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_indirect_calls(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::CallIndirect { callee, args, .. } = &instruction.kind else {
                continue;
            };
            let callee_type = function.value_types.get(callee);
            let argument_types = args
                .iter()
                .map(|argument| function.value_types.get(argument).cloned())
                .collect::<Option<Vec<_>>>();
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let valid = match callee_type {
                Some(Ty::Function { params, result, .. }) => {
                    argument_types.as_ref() == Some(params)
                        && match instruction.result {
                            Some(_) => result_type == Some(result.as_ref()),
                            None => result.as_ref() == &Ty::Void,
                        }
                }
                _ => false,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-indirect-call",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} indirect call uses callee {callee:?} with type {callee_type:?}, arguments {args:?} with types {argument_types:?}, and result {:?} with type {result_type:?}; expected a function-typed callee with exact argument and result types",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_closure_calls(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::CallClosure { closure, args, .. } = &instruction.kind else {
                continue;
            };
            let closure_type = function.value_types.get(closure);
            let argument_types = args
                .iter()
                .map(|argument| function.value_types.get(argument).cloned())
                .collect::<Option<Vec<_>>>();
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let compatible_body = match closure_type {
                Some(Ty::Closure { params, result }) => {
                    function.closures.values().any(|candidate| {
                        !candidate.function_pointer
                            && candidate.return_type == **result
                            && candidate.params.len() == params.len()
                            && candidate
                                .params
                                .iter()
                                .zip(params)
                                .all(|(local, expected)| {
                                    function
                                        .locals
                                        .get(local)
                                        .is_some_and(|local| &local.ty == expected)
                                })
                    })
                }
                _ => false,
            };
            let valid = match closure_type {
                Some(Ty::Closure { params, result }) => {
                    argument_types.as_ref() == Some(params)
                        && if result.as_ref() == &Ty::Void {
                            instruction.result.is_none()
                        } else {
                            instruction.result.is_some() && result_type == Some(result.as_ref())
                        }
                        && compatible_body
                }
                _ => false,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-closure-call",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} local closure call uses callee {closure:?} with type {closure_type:?}, arguments {args:?} with types {argument_types:?}, and result {:?} with type {result_type:?}; compatible local body={compatible_body}, expected a closure-typed callee with exact argument/result types and a compatible function-local closure body",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn make_closure_capture_type(field: &fir::FirClosureField) -> Ty {
    match field.mode {
        CaptureMode::Value => field.ty.clone(),
        CaptureMode::SharedReference => Ty::Reference {
            mutable: false,
            inner: Box::new(field.ty.clone()),
        },
        CaptureMode::MutableReference => Ty::Reference {
            mutable: true,
            inner: Box::new(field.ty.clone()),
        },
    }
}

fn verify_make_closures(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::MakeClosure { closure, captures } = &instruction.kind else {
                continue;
            };
            let metadata = function.closures.get(closure);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let parameter_types = metadata.and_then(|metadata| {
                metadata
                    .params
                    .iter()
                    .map(|local| function.locals.get(local).map(|local| local.ty.clone()))
                    .collect::<Option<Vec<_>>>()
            });
            let expected_result = metadata.and_then(|metadata| {
                parameter_types.as_ref().map(|params| {
                    if metadata.function_pointer {
                        Ty::Function {
                            params: params.clone(),
                            result: Box::new(metadata.return_type.clone()),
                            named_arguments: false,
                        }
                    } else {
                        Ty::Closure {
                            params: params.clone(),
                            result: Box::new(metadata.return_type.clone()),
                        }
                    }
                })
            });
            let capture_types = captures
                .iter()
                .map(|capture| function.value_types.get(capture).cloned())
                .collect::<Option<Vec<_>>>();
            let expected_capture_types = metadata.map(|metadata| {
                metadata
                    .captures
                    .iter()
                    .map(make_closure_capture_type)
                    .collect::<Vec<_>>()
            });
            let metadata_matches = metadata.is_some_and(|metadata| {
                metadata.id == *closure
                    && (!metadata.function_pointer || metadata.captures.is_empty())
            });
            let valid = metadata_matches
                && instruction.result.is_some()
                && result_type == expected_result.as_ref()
                && capture_types == expected_capture_types;
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-make-closure",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} MakeClosure {closure:?} has metadata {metadata:?}, result {:?} with type {result_type:?} (expected {expected_result:?}), and captures {captures:?} with types {capture_types:?} (expected {expected_capture_types:?}); expected an exact function-local body signature and capture environment",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn closure_capture_root(place: &fir::FirPlace) -> Option<(ExprId, u32, bool)> {
    match place {
        fir::FirPlace::ClosureCapture { closure, index } => Some((*closure, *index, true)),
        fir::FirPlace::Field { base, .. } | fir::FirPlace::Index { base, .. } => {
            closure_capture_root(base).map(|(closure, index, _)| (closure, index, false))
        }
        _ => None,
    }
}

fn verify_closure_capture_places(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (place, operation, write, actual_type, result_shape_ok, address_mutable) =
                match &instruction.kind {
                    FirInstructionKind::Load { place } => (
                        place,
                        "load",
                        false,
                        instruction
                            .result
                            .and_then(|result| function.value_types.get(&result)),
                        instruction.result.is_some(),
                        None,
                    ),
                    FirInstructionKind::Store { place, value } => (
                        place,
                        "store",
                        true,
                        function.value_types.get(value),
                        instruction.result.is_none(),
                        None,
                    ),
                    FirInstructionKind::AddressOf { place, mutable } => (
                        place,
                        "address",
                        *mutable,
                        instruction
                            .result
                            .and_then(|result| function.value_types.get(&result)),
                        instruction.result.is_some(),
                        Some(*mutable),
                    ),
                    _ => continue,
                };
            let Some((closure, index, direct)) = closure_capture_root(place) else {
                continue;
            };
            let metadata = function.closures.get(&closure);
            let field = metadata.and_then(|metadata| metadata.captures.get(index as usize));
            let expected_type = field.map(|field| match address_mutable {
                Some(mutable) => Ty::Reference {
                    mutable,
                    inner: Box::new(field.ty.clone()),
                },
                None => field.ty.clone(),
            });
            let valid = block.closure == Some(closure)
                && field.is_some()
                && (!write
                    || !matches!(
                        field.map(|field| field.mode),
                        Some(CaptureMode::SharedReference)
                    ))
                && (!direct || (result_shape_ok && actual_type == expected_type.as_ref()));
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-closure-capture",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} {operation} uses closure-capture place {place:?} while active closure is {:?}; metadata={metadata:?}, field={field:?}, direct={direct}, write={write}, actual type={actual_type:?}, expected direct type={expected_type:?}; expected an in-range capture owned by the active function-local closure, exact direct access types, and no write through a shared capture",
                        function.owner, block.id, block.closure
                    ),
                ));
            }
        }
    }
}

fn verify_direct_local_loads(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::Load {
                place: fir::FirPlace::Local { local },
            } = &instruction.kind
            else {
                continue;
            };
            let local_data = function.locals.get(local);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let valid = instruction.result.is_some()
                && local_data.is_some_and(|local_data| result_type == Some(&local_data.ty));
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-local-load",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} loads local {local:?} with metadata {local_data:?} into result {:?} with type {result_type:?}; expected a result, an existing local, and the exact declared local type",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_local_stores(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::Store {
                place: fir::FirPlace::Local { local },
                value,
            } = &instruction.kind
            else {
                continue;
            };
            let local_data = function.locals.get(local);
            let value_type = function.value_types.get(value);
            let valid = instruction.result.is_none()
                && local_data.is_some_and(|local_data| value_type == Some(&local_data.ty));
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-local-store",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} stores value {value:?} with type {value_type:?} into local {local:?} with metadata {local_data:?} and result {:?}; expected no result, an existing local, and the exact declared local type",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_local_addresses(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::AddressOf {
                place: fir::FirPlace::Local { local },
                mutable,
            } = &instruction.kind
            else {
                continue;
            };
            let local_data = function.locals.get(local);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let expected_type = local_data.map(|local_data| Ty::Reference {
                mutable: *mutable,
                inner: Box::new(local_data.ty.clone()),
            });
            let valid = local_data.is_some_and(|local_data| !*mutable || local_data.mutable)
                && instruction.result.is_some()
                && result_type == expected_type.as_ref();
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-local-address",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} takes an address of local {local:?} with requested mutable={mutable}, metadata {local_data:?}, result {:?}, and result type {result_type:?}; expected an existing local, mutable storage for a mutable address, a result, and exact reference type {expected_type:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_dereference_addresses(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::AddressOf { place, mutable } = &instruction.kind else {
                continue;
            };
            let (address, raw_volatile) = match place {
                fir::FirPlace::Deref { address } => (*address, None),
                fir::FirPlace::RawDeref {
                    address, volatile, ..
                } => (*address, Some(*volatile)),
                _ => continue,
            };
            let address_type = function.value_types.get(&address);
            let pointee = match (raw_volatile, address_type) {
                (
                    None,
                    Some(Ty::Reference {
                        mutable: source_mutable,
                        inner,
                    }),
                ) if !*mutable || *source_mutable => Some(inner.as_ref()),
                (
                    Some(place_volatile),
                    Some(Ty::Pointer {
                        volatile: pointer_volatile,
                        inner,
                    }),
                ) if place_volatile == *pointer_volatile => Some(inner.as_ref()),
                _ => None,
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let expected_type = pointee.map(|pointee| Ty::Reference {
                mutable: *mutable,
                inner: Box::new(pointee.clone()),
            });
            let valid = instruction.result.is_some()
                && pointee.is_some()
                && result_type == expected_type.as_ref();
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-dereference-address",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} takes an address of dereference place {place:?} from value {address:?} with type {address_type:?}, requested mutable={mutable}, result {:?}, and result type {result_type:?}; expected a compatible reference/pointer source, matching raw volatility, mutable source reference for a mutable safe address, a result, and exact reference type {expected_type:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_safe_dereferences(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (place, operation, write, actual_type, result_shape_ok) = match &instruction.kind {
                FirInstructionKind::Load { place } => (
                    place,
                    "load",
                    false,
                    instruction
                        .result
                        .and_then(|result| function.value_types.get(&result)),
                    instruction.result.is_some(),
                ),
                FirInstructionKind::Store { place, value } => (
                    place,
                    "store",
                    true,
                    function.value_types.get(value),
                    instruction.result.is_none(),
                ),
                _ => continue,
            };
            let fir::FirPlace::Deref { address } = place else {
                continue;
            };
            let address_type = function.value_types.get(address);
            let pointee = match address_type {
                Some(Ty::Reference { mutable, inner }) if !write || *mutable => {
                    Some(inner.as_ref())
                }
                _ => None,
            };
            let valid = result_shape_ok && pointee.is_some() && actual_type == pointee;
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-safe-dereference",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} safe {operation} uses reference value {address:?} with type {address_type:?}, actual value type {actual_type:?}, and result {:?}; expected a reference source, mutable reference for a store, correct result shape, and the exact pointee type {pointee:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_direct_raw_dereferences(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (place, operation, actual_type, result_shape_ok) = match &instruction.kind {
                FirInstructionKind::Load { place } => (
                    place,
                    "load",
                    instruction
                        .result
                        .and_then(|result| function.value_types.get(&result)),
                    instruction.result.is_some(),
                ),
                FirInstructionKind::Store { place, value } => (
                    place,
                    "store",
                    function.value_types.get(value),
                    instruction.result.is_none(),
                ),
                _ => continue,
            };
            let fir::FirPlace::RawDeref {
                address, volatile, ..
            } = place
            else {
                continue;
            };
            let address_type = function.value_types.get(address);
            let pointee = match address_type {
                Some(Ty::Pointer {
                    volatile: pointer_volatile,
                    inner,
                }) if volatile == pointer_volatile => Some(inner.as_ref()),
                _ => None,
            };
            let valid = result_shape_ok && pointee.is_some() && actual_type == pointee;
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-raw-dereference",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} raw {operation} uses pointer value {address:?} with type {address_type:?}, place volatility={volatile}, actual value type {actual_type:?}, and result {:?}; expected a pointer source with matching volatility, correct result shape, and the exact pointee type {pointee:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_pointer_offsets(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::PointerOffset {
                pointer, offset, ..
            } = &instruction.kind
            else {
                continue;
            };
            let pointer_type = function.value_types.get(pointer);
            let offset_type = function.value_types.get(offset);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let valid = matches!(pointer_type, Some(Ty::Pointer { .. }))
                && matches!(offset_type, Some(Ty::Byte | Ty::Int { .. }))
                && instruction.result.is_some()
                && result_type == pointer_type;
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-pointer-offset",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} offsets pointer value {pointer:?} with type {pointer_type:?} by value {offset:?} with type {offset_type:?}, producing result {:?} with type {result_type:?}; expected a pointer base, concrete integer offset, result value, and the exact base-pointer result type",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_pointer_conversions(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::PointerConvert {
                value,
                target,
                operation,
                ..
            } = &instruction.kind
            else {
                continue;
            };
            let source_type = function.value_types.get(value);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let operation_matches = match operation {
                crate::typecheck::UnsafeOperationKind::PointerToInteger => {
                    matches!(source_type, Some(Ty::Pointer { .. }))
                        && matches!(target, Ty::Byte | Ty::Int { .. })
                }
                crate::typecheck::UnsafeOperationKind::IntegerToPointer => {
                    matches!(source_type, Some(Ty::Byte | Ty::Int { .. }))
                        && matches!(target, Ty::Pointer { .. })
                }
                crate::typecheck::UnsafeOperationKind::PointerReinterpret => {
                    matches!(source_type, Some(Ty::Pointer { .. }))
                        && matches!(target, Ty::Pointer { .. })
                        && source_type != Some(target)
                }
                crate::typecheck::UnsafeOperationKind::RawDereference { .. }
                | crate::typecheck::UnsafeOperationKind::PointerOffset { .. } => false,
            };
            let valid =
                instruction.result.is_some() && result_type == Some(target) && operation_matches;
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-pointer-convert",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} converts value {value:?} with type {source_type:?} using operation {operation:?} to target {target:?}, producing result {:?} with type {result_type:?}; expected a result matching the target and exact pointer-to-integer, integer-to-pointer, or distinct-pointer endpoint types for the operation tag",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_array_construction(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::MakeArray { items } = &instruction.kind else {
                continue;
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let (element_type, declared_length) = match result_type {
                Some(Ty::Array {
                    element,
                    length: Some(length),
                }) => (Some(element.as_ref()), Some(*length)),
                _ => (None, None),
            };
            let item_types = items
                .iter()
                .map(|item| function.value_types.get(item))
                .collect::<Vec<_>>();
            let valid = instruction.result.is_some()
                && declared_length == Some(items.len() as u64)
                && element_type.is_some()
                && item_types
                    .iter()
                    .all(|item_type| *item_type == element_type);
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-make-array",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} constructs array result {:?} with type {result_type:?} from items {items:?} with types {item_types:?}; expected a fixed-array result, declared length matching the item count, and every item to have the exact element type {element_type:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_lengths(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::Len { value } = &instruction.kind else {
                continue;
            };
            let source_type = function.value_types.get(value);
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let usize_type = Ty::Int {
                signed: false,
                width: IntWidth::Pointer,
            };
            let valid_source = matches!(
                source_type,
                Some(
                    Ty::Array {
                        length: Some(_),
                        ..
                    } | Ty::Slice { .. }
                        | Ty::Str
                )
            );
            let valid =
                valid_source && instruction.result.is_some() && result_type == Some(&usize_type);
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-len",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} computes length from value {value:?} with type {source_type:?} into result {:?} with type {result_type:?}; expected a fixed array, slice or string input and a usize result",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_indexing(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    let usize_type = Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    };
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            match &instruction.kind {
                FirInstructionKind::BoundsCheck { index, len } => {
                    let index_type = function.value_types.get(index);
                    let length_type = function.value_types.get(len);
                    let valid = instruction.result.is_none()
                        && index_type == Some(&usize_type)
                        && length_type == Some(&usize_type);
                    if !valid {
                        diagnostics.push(diagnostic(
                            instruction.span,
                            "fir/verify-bounds-check",
                            format!(
                                "FIR function {:?} block {:?} instruction {instruction_index} bounds-checks index {index:?} with type {index_type:?} against length {len:?} with type {length_type:?} and result {:?}; expected usize operands and no result",
                                function.owner, block.id, instruction.result
                            ),
                        ));
                    }
                }
                FirInstructionKind::IndexUnchecked { base, index } => {
                    let base_type = function.value_types.get(base);
                    let index_type = function.value_types.get(index);
                    let result_type = instruction
                        .result
                        .and_then(|result| function.value_types.get(&result));
                    let expected_result = match base_type {
                        Some(
                            Ty::Array {
                                element,
                                length: Some(_),
                            }
                            | Ty::Slice { element, .. },
                        ) => Some(element.as_ref().clone()),
                        Some(Ty::Str) => Some(Ty::Int {
                            signed: false,
                            width: IntWidth::W8,
                        }),
                        _ => None,
                    };
                    let valid = instruction.result.is_some()
                        && index_type == Some(&usize_type)
                        && result_type == expected_result.as_ref();
                    if !valid {
                        diagnostics.push(diagnostic(
                            instruction.span,
                            "fir/verify-index-unchecked",
                            format!(
                                "FIR function {:?} block {:?} instruction {instruction_index} indexes base {base:?} with type {base_type:?} using index {index:?} with type {index_type:?} into result {:?} with type {result_type:?}; expected a fixed array, slice or string base, a usize index, and exact element result type {expected_result:?}",
                                function.owner, block.id, instruction.result
                            ),
                        ));
                    }
                }
                _ => {}
            }
        }
    }
}

fn verify_string_constants(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (kind, expected_type) = match &instruction.kind {
                FirInstructionKind::Const {
                    value: fir::FirConst::String { .. },
                } => ("string", Ty::Str),
                FirInstructionKind::Const {
                    value: fir::FirConst::CString { .. },
                } => (
                    "C-string",
                    Ty::Pointer {
                        volatile: false,
                        inner: Box::new(Ty::Byte),
                    },
                ),
                _ => continue,
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let valid = instruction.result.is_some() && result_type == Some(&expected_type);
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-string-constant",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} produces {kind} constant result {:?} with type {result_type:?}; expected exact result type {expected_type:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_zero_payload_producers(
    function: &fir::FirFunction,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let (kind, valid_type) = match &instruction.kind {
                FirInstructionKind::Unit => ("unit", Some(&Ty::Void)),
                FirInstructionKind::MakeNone => ("make-none", None),
                _ => continue,
            };
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let valid = instruction.result.is_some()
                && match valid_type {
                    Some(expected) => result_type == Some(expected),
                    None => matches!(result_type, Some(Ty::Optional { .. })),
                };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-zero-payload-producer",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} produces {kind} result {:?} with type {result_type:?}; expected {}",
                        function.owner,
                        block.id,
                        instruction.result,
                        if valid_type.is_some() {
                            "an exact void result"
                        } else {
                            "an optional result"
                        }
                    ),
                ));
            }
        }
    }
}

fn verify_option_operations(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let (operation, input, input_type, expected_result, valid) = match &instruction.kind {
                FirInstructionKind::MakeSome { value } => {
                    let input_type = function.value_types.get(value);
                    let expected_result = input_type.map(|input| Ty::Optional {
                        inner: Box::new(input.clone()),
                    });
                    (
                        "MakeSome",
                        *value,
                        input_type,
                        expected_result.clone(),
                        instruction.result.is_some()
                            && input_type.is_some()
                            && result_type == expected_result.as_ref(),
                    )
                }
                FirInstructionKind::OptionIsSome { value } => {
                    let input_type = function.value_types.get(value);
                    (
                        "OptionIsSome",
                        *value,
                        input_type,
                        Some(Ty::Bool),
                        instruction.result.is_some()
                            && matches!(input_type, Some(Ty::Optional { .. }))
                            && result_type == Some(&Ty::Bool),
                    )
                }
                FirInstructionKind::OptionUnwrap { value } => {
                    let input_type = function.value_types.get(value);
                    let expected_result = match input_type {
                        Some(Ty::Optional { inner }) => Some(inner.as_ref().clone()),
                        _ => None,
                    };
                    (
                        "OptionUnwrap",
                        *value,
                        input_type,
                        expected_result.clone(),
                        instruction.result.is_some()
                            && expected_result.is_some()
                            && result_type == expected_result.as_ref(),
                    )
                }
                _ => continue,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-option-operation",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} {operation} uses value {input:?} with type {input_type:?} and produces result {:?} with type {result_type:?}; expected an optional operation with exact result type {expected_result:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_result_operations(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let (operation, input, input_type, expected_result, valid) = match &instruction.kind {
                FirInstructionKind::MakeResultOk { value } => {
                    let input_type = function.value_types.get(value);
                    let expected_result = match result_type {
                        Some(Ty::Result { ok, .. }) if Some(ok.as_ref()) == input_type => {
                            result_type.cloned()
                        }
                        _ => None,
                    };
                    (
                        "MakeResultOk",
                        *value,
                        input_type,
                        expected_result,
                        instruction.result.is_some()
                            && matches!(
                                result_type,
                                Some(Ty::Result { ok, .. })
                                    if Some(ok.as_ref()) == input_type
                            ),
                    )
                }
                FirInstructionKind::MakeResultErr { error } => {
                    let input_type = function.value_types.get(error);
                    let expected_result = match result_type {
                        Some(Ty::Result { error, .. }) if Some(error.as_ref()) == input_type => {
                            result_type.cloned()
                        }
                        _ => None,
                    };
                    (
                        "MakeResultErr",
                        *error,
                        input_type,
                        expected_result,
                        instruction.result.is_some()
                            && matches!(
                                result_type,
                                Some(Ty::Result { error, .. })
                                    if Some(error.as_ref()) == input_type
                            ),
                    )
                }
                FirInstructionKind::ResultIsOk { value } => {
                    let input_type = function.value_types.get(value);
                    (
                        "ResultIsOk",
                        *value,
                        input_type,
                        Some(Ty::Bool),
                        instruction.result.is_some()
                            && matches!(input_type, Some(Ty::Result { .. }))
                            && result_type == Some(&Ty::Bool),
                    )
                }
                FirInstructionKind::ResultUnwrapOk { value }
                | FirInstructionKind::ResultUnwrapErr { value } => {
                    let input_type = function.value_types.get(value);
                    let (operation, expected_result) = match (&instruction.kind, input_type) {
                        (
                            FirInstructionKind::ResultUnwrapOk { .. },
                            Some(Ty::Result { ok, .. }),
                        ) => ("ResultUnwrapOk", Some(ok.as_ref().clone())),
                        (
                            FirInstructionKind::ResultUnwrapErr { .. },
                            Some(Ty::Result { error, .. }),
                        ) => ("ResultUnwrapErr", Some(error.as_ref().clone())),
                        (FirInstructionKind::ResultUnwrapOk { .. }, _) => ("ResultUnwrapOk", None),
                        (FirInstructionKind::ResultUnwrapErr { .. }, _) => {
                            ("ResultUnwrapErr", None)
                        }
                        _ => unreachable!(),
                    };
                    (
                        operation,
                        *value,
                        input_type,
                        expected_result.clone(),
                        instruction.result.is_some()
                            && expected_result.is_some()
                            && result_type == expected_result.as_ref(),
                    )
                }
                _ => continue,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-result-operation",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} {operation} uses value {input:?} with type {input_type:?} and produces result {:?} with type {result_type:?}; expected a result operation with exact result type {expected_result:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn verify_variant_operations(function: &fir::FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let result_type = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let (operation, input_type, expected_result, valid) = match &instruction.kind {
                FirInstructionKind::Variant { ty, .. } => (
                    "Variant",
                    None,
                    Some(ty),
                    instruction.result.is_some()
                        && matches!(ty, Ty::Nominal(_))
                        && result_type == Some(ty),
                ),
                FirInstructionKind::VariantIs { value, .. } => {
                    let input_type = function.value_types.get(value);
                    (
                        "VariantIs",
                        input_type,
                        Some(&Ty::Bool),
                        instruction.result.is_some()
                            && matches!(input_type, Some(Ty::Nominal(_)))
                            && result_type == Some(&Ty::Bool),
                    )
                }
                _ => continue,
            };
            if !valid {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-variant-operation",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} {operation} uses input type {input_type:?} and produces result {:?} with type {result_type:?}; expected a nominal variant operation with exact result type {expected_result:?}",
                        function.owner, block.id, instruction.result
                    ),
                ));
            }
        }
    }
}

fn make_aggregate_contract_issue(
    function: &fir::FirFunction,
    definitions: &TypeDefinitionTable,
    result: Option<fir::FirValueId>,
    declared_ty: &Ty,
    variant_name: Option<&str>,
    fields: &[(String, fir::FirValueId)],
) -> Option<String> {
    let result_ty = result.and_then(|value| function.value_types.get(&value));
    if result_ty != Some(declared_ty) {
        return Some(format!(
            "declared type {declared_ty:?} does not match result type {result_ty:?}"
        ));
    }
    let Ty::Nominal(owner) = declared_ty else {
        return Some(format!("declared type {declared_ty:?} is not nominal"));
    };
    let Some(definition) = definitions.get(owner) else {
        return Some(format!("declared type {owner:?} has no type definition"));
    };
    let declared_fields = match &definition.kind {
        TypeDefinitionKind::Struct { fields } => {
            if let Some(name) = variant_name {
                return Some(format!(
                    "struct construction unexpectedly names variant `{name}`"
                ));
            }
            fields
        }
        TypeDefinitionKind::Tagged { variants } => {
            let Some(name) = variant_name else {
                return Some("tagged construction is missing its variant".into());
            };
            let Some(variant) = variants.iter().find(|variant| variant.name == name) else {
                return Some(format!("unknown variant `{name}` for type {declared_ty:?}"));
            };
            &variant.fields
        }
        _ => return Some(format!("declared type {declared_ty:?} is not an aggregate")),
    };

    let mut seen = BTreeSet::new();
    for (name, value) in fields {
        if !seen.insert(name.as_str()) {
            return Some(format!("field `{name}` is supplied more than once"));
        }
        let Some(declared) = declared_fields.iter().find(|field| field.name == *name) else {
            return Some(format!("field `{name}` is not declared"));
        };
        let payload_ty = function.value_types.get(value);
        if payload_ty != Some(&declared.ty) {
            return Some(format!(
                "field `{name}` has payload type {payload_ty:?}, expected {:?}",
                declared.ty
            ));
        }
    }
    declared_fields
        .iter()
        .find(|field| !seen.contains(field.name.as_str()))
        .map(|field| format!("declared field `{}` is missing", field.name))
}

fn verify_make_aggregates(
    function: &fir::FirFunction,
    definitions: &TypeDefinitionTable,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::MakeAggregate {
                ty,
                variant,
                fields,
            } = &instruction.kind
            else {
                continue;
            };
            if let Some(issue) = make_aggregate_contract_issue(
                function,
                definitions,
                instruction.result,
                ty,
                variant.as_deref(),
                fields,
            ) {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-make-aggregate",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} MakeAggregate violates its type-definition contract: {issue}",
                        function.owner, block.id
                    ),
                ));
            }
        }
    }
}

fn declared_field_type(
    definitions: &TypeDefinitionTable,
    base_ty: &Ty,
    field_name: &str,
    visited: &mut BTreeSet<DefId>,
) -> Result<Ty, String> {
    if *base_ty == Ty::Str {
        return match field_name {
            "data" => Ok(Ty::Pointer {
                volatile: false,
                inner: Box::new(Ty::Int {
                    signed: false,
                    width: IntWidth::W8,
                }),
            }),
            "len" => Ok(Ty::Int {
                signed: false,
                width: IntWidth::Pointer,
            }),
            _ => Err(format!("unknown str field `{field_name}`")),
        };
    }

    let Ty::Nominal(owner) = base_ty else {
        return Err(format!("base type {base_ty:?} is not field-bearing"));
    };
    if !visited.insert(*owner) {
        return Err(format!("type {owner:?} contains a cyclic field alias"));
    }
    let Some(definition) = definitions.get(owner) else {
        return Err(format!("base type {owner:?} has no type definition"));
    };
    match &definition.kind {
        TypeDefinitionKind::Struct { fields } => fields
            .iter()
            .find(|field| field.name == field_name)
            .map(|field| field.ty.clone())
            .ok_or_else(|| format!("field `{field_name}` is not declared")),
        TypeDefinitionKind::Alias { target }
        | TypeDefinitionKind::Distinct { underlying: target } => {
            declared_field_type(definitions, target, field_name, visited)
        }
        TypeDefinitionKind::Tagged { variants } => {
            let mut result = None;
            for field in variants
                .iter()
                .flat_map(|variant| &variant.fields)
                .filter(|field| field.name == field_name)
            {
                if result.as_ref().is_some_and(|ty| ty != &field.ty) {
                    return Err(format!(
                        "field `{field_name}` has variant-dependent types"
                    ));
                }
                result = Some(field.ty.clone());
            }
            result.ok_or_else(|| format!("field `{field_name}` is not declared"))
        }
        _ => Err(format!("base type {base_ty:?} is not field-bearing")),
    }
}

fn verify_extract_fields(
    function: &fir::FirFunction,
    definitions: &TypeDefinitionTable,
    diagnostics: &mut Vec<FirDiagnostic>,
) {
    for block in &function.blocks {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            let FirInstructionKind::ExtractField { base, field } = &instruction.kind else {
                continue;
            };
            let base_ty = function.value_types.get(base);
            let result_ty = instruction
                .result
                .and_then(|result| function.value_types.get(&result));
            let issue = match base_ty {
                Some(base_ty) => {
                    match declared_field_type(definitions, base_ty, field, &mut BTreeSet::new()) {
                        Ok(expected) if result_ty == Some(&expected) => None,
                        Ok(expected) => Some(format!(
                            "result type {result_ty:?} does not match declared field type {expected:?}"
                        )),
                        Err(issue) => Some(issue),
                    }
                }
                None => Some(format!("base value {base:?} has no type")),
            };
            if let Some(issue) = issue {
                diagnostics.push(diagnostic(
                    instruction.span,
                    "fir/verify-extract-field",
                    format!(
                        "FIR function {:?} block {:?} instruction {instruction_index} ExtractField `{field}` violates its type-definition contract: {issue}",
                        function.owner, block.id
                    ),
                ));
            }
        }
    }
}

pub fn verify_fir_module(module: &FirModule) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();

    for owner in module.globals.keys() {
        if module.functions.contains_key(owner) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-definition-namespace",
                format!("definition {owner:?} appears as both a function and a global"),
            ));
        }
    }

    for (owner, global) in &module.globals {
        if global.owner != *owner || !type_is_concrete(&global.ty) {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global",
                format!("global {owner:?} has invalid owner/type metadata"),
            ));
        }
        if let Some(constant) = &global.constant {
            if !global_constant_matches_type(constant, &global.ty) {
                diagnostics.push(diagnostic(
                    Span::new(0, 0),
                    "fir/verify-global-constant",
                    format!(
                        "global {owner:?} constant {constant:?} does not match type {:?}",
                        global.ty
                    ),
                ));
            }
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
        if global.constant.is_some() {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-initialization",
                format!(
                    "global {owner:?} has both a compile-time constant and a runtime initializer"
                ),
            ));
        }
        if initializer.owner != *owner || initializer.function.return_type != global.ty {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} has invalid owner/result type"),
            ));
        }
        if initializer.function.owner != *owner {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init-function-owner",
                format!(
                    "runtime initializer {owner:?} function owner {:?} differs from its global owner",
                    initializer.function.owner
                ),
            ));
        }
        if !initializer.function.params.is_empty() {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-global-init-signature",
                format!(
                    "runtime initializer {owner:?} takes {} parameters; expected none",
                    initializer.function.params.len()
                ),
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
        verify_static_data_addresses(&initializer.function, module, &mut diagnostics);
        verify_global_accesses(&initializer.function, module, &mut diagnostics);
        verify_bitfield_width_operations(&initializer.function, &mut diagnostics);
        verify_function_references(&initializer.function, module, &mut diagnostics);
        verify_direct_calls(&initializer.function, module, &mut diagnostics);
        verify_indirect_calls(&initializer.function, &mut diagnostics);
        verify_closure_calls(&initializer.function, &mut diagnostics);
        verify_make_closures(&initializer.function, &mut diagnostics);
        verify_closure_capture_places(&initializer.function, &mut diagnostics);
        verify_direct_local_loads(&initializer.function, &mut diagnostics);
        verify_direct_local_stores(&initializer.function, &mut diagnostics);
        verify_direct_local_addresses(&initializer.function, &mut diagnostics);
        verify_direct_dereference_addresses(&initializer.function, &mut diagnostics);
        verify_direct_safe_dereferences(&initializer.function, &mut diagnostics);
        verify_direct_raw_dereferences(&initializer.function, &mut diagnostics);
        verify_pointer_offsets(&initializer.function, &mut diagnostics);
        verify_pointer_conversions(&initializer.function, &mut diagnostics);
        verify_array_construction(&initializer.function, &mut diagnostics);
        verify_lengths(&initializer.function, &mut diagnostics);
        verify_indexing(&initializer.function, &mut diagnostics);
        verify_string_constants(&initializer.function, &mut diagnostics);
        verify_zero_payload_producers(&initializer.function, &mut diagnostics);
        verify_option_operations(&initializer.function, &mut diagnostics);
        verify_result_operations(&initializer.function, &mut diagnostics);
        verify_variant_operations(&initializer.function, &mut diagnostics);
    }

    for (owner, function) in &module.functions {
        if function.owner != *owner {
            diagnostics.push(diagnostic(
                Span::new(0, 0),
                "fir/verify-function-owner",
                format!(
                    "function map key {owner:?} differs from function owner {:?}",
                    function.owner
                ),
            ));
        }
        diagnostics.extend(fir::verify_fir_function(function));
        verify_no_poison(function, &mut diagnostics);
        verify_static_data_addresses(function, module, &mut diagnostics);
        verify_global_accesses(function, module, &mut diagnostics);
        verify_bitfield_width_operations(function, &mut diagnostics);
        verify_function_references(function, module, &mut diagnostics);
        verify_direct_calls(function, module, &mut diagnostics);
        verify_indirect_calls(function, &mut diagnostics);
        verify_closure_calls(function, &mut diagnostics);
        verify_make_closures(function, &mut diagnostics);
        verify_closure_capture_places(function, &mut diagnostics);
        verify_direct_local_loads(function, &mut diagnostics);
        verify_direct_local_stores(function, &mut diagnostics);
        verify_direct_local_addresses(function, &mut diagnostics);
        verify_direct_dereference_addresses(function, &mut diagnostics);
        verify_direct_safe_dereferences(function, &mut diagnostics);
        verify_direct_raw_dereferences(function, &mut diagnostics);
        verify_pointer_offsets(function, &mut diagnostics);
        verify_pointer_conversions(function, &mut diagnostics);
        verify_array_construction(function, &mut diagnostics);
        verify_lengths(function, &mut diagnostics);
        verify_indexing(function, &mut diagnostics);
        verify_string_constants(function, &mut diagnostics);
        verify_zero_payload_producers(function, &mut diagnostics);
        verify_option_operations(function, &mut diagnostics);
        verify_result_operations(function, &mut diagnostics);
        verify_variant_operations(function, &mut diagnostics);
    }

    diagnostics
}

pub fn verify_fir_module_with_types(
    module: &FirModule,
    definitions: &TypeDefinitionTable,
) -> Vec<FirDiagnostic> {
    let mut diagnostics = verify_fir_module(module);
    for initializer in module.global_initializers.values() {
        verify_make_aggregates(&initializer.function, definitions, &mut diagnostics);
        verify_extract_fields(&initializer.function, definitions, &mut diagnostics);
    }
    for function in module.functions.values() {
        verify_make_aggregates(function, definitions, &mut diagnostics);
        verify_extract_fields(function, definitions, &mut diagnostics);
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
