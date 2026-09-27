use std::collections::BTreeMap;

use crate::{
    ast::{FdnValue, Span},
    body_hir::{BodyHirOutput, HirExpr, HirExprKind},
    fir::{self, FirDiagnostic, FirInstructionKind, FirModule, FirOutput},
    typecheck::{ConstValue, IntWidth, Ty, TypeCheckOutput, TypedBody, TypedExpr, TypedExprKind},
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
            let expected = module.functions.get(target).and_then(|callee| {
                callee
                    .params
                    .iter()
                    .map(|parameter| callee.locals.get(parameter).map(|local| local.ty.clone()))
                    .collect::<Option<Vec<_>>>()
                    .map(|params| (params, callee.return_type.clone()))
            });
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
