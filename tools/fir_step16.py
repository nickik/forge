from pathlib import Path


def replace(path, old, new):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))

# ----- typed HIR: remove the last source-only semantic reader form -----
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''    ResolvedContext {
        slot: ContextSlot,
        hir: HirExpr,
    },
    ResolvedTry {''',
    '''    ResolvedContext {
        slot: ContextSlot,
        hir: HirExpr,
    },
    ResolvedDuration {
        value: String,
        hir: HirExpr,
    },
    ResolvedTry {''',
)
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        let mut resolved_context: Option<ContextSlot> = None;
        let mut resolved_bitfield: Option<TypedBitFieldAccess> = None;''',
    '''        let mut resolved_context: Option<ContextSlot> = None;
        let mut resolved_duration: Option<String> = None;
        let mut resolved_bitfield: Option<TypedBitFieldAccess> = None;''',
)
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''            HirExprKind::ReaderForm { tag, value } => {
                if tag == "duration" && matches!(value, ast::FdnValue::String { .. }) {
                    Ty::Duration
                } else {
                    Ty::Unknown
                }
            }''',
    '''            HirExprKind::ReaderForm { tag, value } => {
                if tag == "duration" {
                    if let ast::FdnValue::String { value } = value {
                        resolved_duration = Some(value.clone());
                        Ty::Duration
                    } else {
                        Ty::Unknown
                    }
                } else {
                    Ty::Unknown
                }
            }''',
)
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        } else if let Some(slot) = resolved_context {
            TypedExprKind::ResolvedContext {
                slot,
                hir: expr.clone(),
            }
        } else if let Some(plan) = resolved_match {''',
    '''        } else if let Some(slot) = resolved_context {
            TypedExprKind::ResolvedContext {
                slot,
                hir: expr.clone(),
            }
        } else if let Some(value) = resolved_duration {
            TypedExprKind::ResolvedDuration {
                value,
                hir: expr.clone(),
            }
        } else if let Some(plan) = resolved_match {''',
)
replace(
    "crates/forge-frontend/src/typecheck_v1.rs",
    '''        | TypedExprKind::ResolvedContext { hir, .. }
        | TypedExprKind::ResolvedTry { hir, .. }''',
    '''        | TypedExprKind::ResolvedContext { hir, .. }
        | TypedExprKind::ResolvedDuration { hir, .. }
        | TypedExprKind::ResolvedTry { hir, .. }''',
)

# ----- FIR lowering: consume resolved duration, verify boundary/module, stable dump -----
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    '''pub fn lower_fir(bodies: &BodyHirOutput, typed: &TypeCheckOutput) -> FirOutput {
    let mut output = FirOutput::default();
''',
    '''pub fn lower_fir(bodies: &BodyHirOutput, typed: &TypeCheckOutput) -> FirOutput {
    let mut output = FirOutput::default();
    output.diagnostics.extend(verify_fir_boundary(bodies, typed));
''',
)
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    '''    output
}

fn function_overflow_mode''',
    '''    output.diagnostics.extend(verify_fir_module(&output.module));
    output
}

pub fn dump_fir_module(module: &FirModule) -> String {
    serde_json::to_string_pretty(module).expect("FIR module serialization")
}

fn function_overflow_mode''',
)
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    '''            TypedExprKind::ResolvedContext { slot, .. } => self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::ContextLoad { slot: *slot },
            ),
            TypedExprKind::ResolvedTry {''',
    '''            TypedExprKind::ResolvedContext { slot, .. } => self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::ContextLoad { slot: *slot },
            ),
            TypedExprKind::ResolvedDuration { value, .. } => self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::Const {
                    value: FirConst::Duration {
                        value: value.clone(),
                    },
                },
            ),
            TypedExprKind::ResolvedTry {''',
)
old_duration = '''            HirExprKind::ReaderForm { tag, value } if ty == Ty::Duration && tag == "duration" => {
                let FdnValue::String { value } = value else {
                    self.diagnostic(
                        expr.span,
                        "fir/duration",
                        "typed duration reader form has non-string payload",
                    );
                    return self.poison(expr.span, ty);
                };
                self.emit_value(
                    expr.span,
                    ty,
                    FirInstructionKind::Const {
                        value: FirConst::Duration {
                            value: value.clone(),
                        },
                    },
                )
            }
'''
replace("crates/forge-frontend/src/fir_v1.rs", old_duration, "")

# FdnValue is no longer needed by FIR itself after duration normalization.
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    "    ast::{BinaryOp, FdnValue, MetadataArg, Span, UnaryOp},",
    "    ast::{BinaryOp, MetadataArg, Span, UnaryOp},",
)

# Append boundary + module verifier helpers before the existing function verifier.
marker = '''pub fn verify_fir_function(function: &FirFunction) -> Vec<FirDiagnostic> {'''
helpers = r'''fn boundary_diag(span: Span, code: &str, message: impl Into<String>) -> FirDiagnostic {
    FirDiagnostic {
        span,
        code: code.into(),
        message: message.into(),
    }
}

fn typed_expr_source(kind: &TypedExprKind) -> &HirExpr {
    match kind {
        TypedExprKind::Source { hir }
        | TypedExprKind::ResolvedCall { hir, .. }
        | TypedExprKind::ResolvedClosure { hir, .. }
        | TypedExprKind::ResolvedContext { hir, .. }
        | TypedExprKind::ResolvedDuration { hir, .. }
        | TypedExprKind::ResolvedTry { hir, .. }
        | TypedExprKind::ResolvedMatch { hir, .. }
        | TypedExprKind::UnsafeOperation { hir, .. }
        | TypedExprKind::ResolvedBitField { hir, .. }
        | TypedExprKind::OptionalPromote { hir, .. } => hir,
    }
}

fn verify_typed_expr_kind(expr: &TypedExpr, kind: &TypedExprKind, diagnostics: &mut Vec<FirDiagnostic>) {
    let hir = typed_expr_source(kind);
    match kind {
        TypedExprKind::Source { .. } => {
            if matches!(
                hir.kind,
                HirExprKind::Context { .. }
                    | HirExprKind::Try { .. }
                    | HirExprKind::Match { .. }
                    | HirExprKind::Closure { .. }
                    | HirExprKind::ReaderForm { .. }
            ) {
                diagnostics.push(boundary_diag(
                    expr.span,
                    "fir/boundary-source",
                    format!("source-only semantic expression reached FIR boundary: {:?}", hir.kind),
                ));
            }
        }
        TypedExprKind::ResolvedCall { .. } if !matches!(hir.kind, HirExprKind::Call { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved call is not backed by call HIR"));
        }
        TypedExprKind::ResolvedClosure { .. } if !matches!(hir.kind, HirExprKind::Closure { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved closure is not backed by closure HIR"));
        }
        TypedExprKind::ResolvedContext { .. } if !matches!(hir.kind, HirExprKind::Context { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved context slot is not backed by context HIR"));
        }
        TypedExprKind::ResolvedDuration { .. }
            if !matches!(&hir.kind, HirExprKind::ReaderForm { tag, .. } if tag == "duration") =>
        {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved duration is not backed by #duration HIR"));
        }
        TypedExprKind::ResolvedTry { .. } if !matches!(hir.kind, HirExprKind::Try { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved try is not backed by try HIR"));
        }
        TypedExprKind::ResolvedMatch { .. } if !matches!(hir.kind, HirExprKind::Match { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved match is not backed by match HIR"));
        }
        TypedExprKind::ResolvedBitField { .. } if !matches!(hir.kind, HirExprKind::Member { .. }) => {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "resolved bit field is not backed by member HIR"));
        }
        TypedExprKind::UnsafeOperation { .. }
            if !matches!(
                hir.kind,
                HirExprKind::Unary { .. } | HirExprKind::Binary { .. } | HirExprKind::TypeCall { .. }
            ) =>
        {
            diagnostics.push(boundary_diag(expr.span, "fir/boundary-shape", "unsafe operation has an invalid HIR shape"));
        }
        TypedExprKind::OptionalPromote { inner, .. } => verify_typed_expr_kind(expr, inner, diagnostics),
        _ => {}
    }
}

fn verify_typed_body_boundary(body: &TypedBody, diagnostics: &mut Vec<FirDiagnostic>) {
    if !fir_type_is_concrete(&body.return_type) {
        diagnostics.push(boundary_diag(
            Span::new(0, 0),
            "fir/boundary-type",
            format!("typed body {:?} has non-concrete return type {:?}", body.owner, body.return_type),
        ));
    }
    for (local, ty) in &body.params {
        if !fir_type_is_concrete(ty) {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("typed parameter {local:?} has non-concrete type {ty:?}"),
            ));
        }
    }
    for (local, ty) in &body.local_types {
        if !fir_type_is_concrete(ty) {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("typed local {local:?} has non-concrete type {ty:?}"),
            ));
        }
    }
    let mut ids = BTreeSet::new();
    for expr in &body.expressions {
        if !ids.insert(expr.id) {
            diagnostics.push(boundary_diag(
                expr.span,
                "fir/boundary-expression",
                format!("typed expression {:?} appears more than once", expr.id),
            ));
        }
        if !fir_type_is_concrete(&expr.ty) {
            diagnostics.push(boundary_diag(
                expr.span,
                "fir/boundary-type",
                format!("typed expression {:?} has non-concrete type {:?}", expr.id, expr.ty),
            ));
        }
        verify_typed_expr_kind(expr, &expr.kind, diagnostics);
    }
}

pub fn verify_fir_boundary(
    bodies: &BodyHirOutput,
    typed: &TypeCheckOutput,
) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();
    for (owner, body) in &bodies.functions {
        let Some(typed_body) = typed.functions.get(owner) else {
            diagnostics.push(boundary_diag(
                body.block.span,
                "fir/boundary-body",
                format!("function {owner:?} has no typed body"),
            ));
            continue;
        };
        verify_typed_body_boundary(typed_body, &mut diagnostics);
    }
    for (owner, ty) in &typed.global_types {
        if !fir_type_is_concrete(ty) {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/boundary-type",
                format!("global {owner:?} has non-concrete type {ty:?}"),
            ));
        }
    }
    for (owner, initializer) in &typed.global_initializers {
        if initializer.owner != *owner || initializer.body.owner != *owner {
            diagnostics.push(boundary_diag(
                initializer.span,
                "fir/boundary-global-init",
                format!("runtime initializer key/owner mismatch for {owner:?}"),
            ));
        }
        if initializer.body.return_type != initializer.ty {
            diagnostics.push(boundary_diag(
                initializer.span,
                "fir/boundary-global-init",
                format!("runtime initializer {owner:?} body/result type mismatch"),
            ));
        }
        verify_typed_body_boundary(&initializer.body, &mut diagnostics);
    }
    diagnostics
}

fn verify_no_poison(function: &FirFunction, diagnostics: &mut Vec<FirDiagnostic>) {
    for block in &function.blocks {
        for instruction in &block.instructions {
            if matches!(instruction.kind, FirInstructionKind::Poison) {
                diagnostics.push(boundary_diag(
                    instruction.span,
                    "fir/verify-poison",
                    format!("successful FIR function {:?} contains Poison", function.owner),
                ));
            }
            if let Some(result) = instruction.result {
                match function.value_types.get(&result) {
                    Some(ty) if fir_type_is_concrete(ty) => {}
                    Some(ty) => diagnostics.push(boundary_diag(
                        instruction.span,
                        "fir/verify-type",
                        format!("value {result:?} has non-concrete type {ty:?}"),
                    )),
                    None => {}
                }
            }
        }
    }
}

pub fn verify_fir_module(module: &FirModule) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();
    for (owner, global) in &module.globals {
        if global.owner != *owner || !fir_type_is_concrete(&global.ty) {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/verify-global",
                format!("global {owner:?} has invalid owner/type metadata"),
            ));
        }
    }

    let mut positions = BTreeMap::new();
    for (index, owner) in module.global_init_order.iter().copied().enumerate() {
        if positions.insert(owner, index).is_some() {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} appears more than once in module order"),
            ));
        }
    }
    if positions.len() != module.global_initializers.len()
        || module.global_initializers.keys().any(|owner| !positions.contains_key(owner))
    {
        diagnostics.push(boundary_diag(
            Span::new(0, 0),
            "fir/verify-global-init",
            "module initializer order does not contain every runtime initializer exactly once",
        ));
    }

    for (owner, initializer) in &module.global_initializers {
        let Some(global) = module.globals.get(owner) else {
            diagnostics.push(boundary_diag(
                Span::new(0, 0),
                "fir/verify-global-init",
                format!("runtime initializer {owner:?} has no global"),
            ));
            continue;
        };
        if initializer.owner != *owner || initializer.function.return_type != global.ty {
            diagnostics.push(boundary_diag(
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
                diagnostics.push(boundary_diag(
                    Span::new(0, 0),
                    "fir/verify-global-init-order",
                    format!("runtime initializer dependency {dependency:?} does not precede {owner:?}"),
                ));
            }
        }
        diagnostics.extend(verify_fir_function(&initializer.function));
        verify_no_poison(&initializer.function, &mut diagnostics);
    }
    for function in module.functions.values() {
        diagnostics.extend(verify_fir_function(function));
        verify_no_poison(function, &mut diagnostics);
    }
    diagnostics
}

pub fn verify_fir_function(function: &FirFunction) -> Vec<FirDiagnostic> {'''
replace("crates/forge-frontend/src/fir_v1.rs", marker, helpers)

# Avoid duplicate per-function verification; whole-module verifier is authoritative.
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    '''        let (function, mut diagnostics) = FunctionLowerer::new(
            &synthetic,
            &plan.body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        diagnostics.extend(verify_fir_function(&function));
        output.diagnostics.extend(diagnostics);''',
    '''        let (function, diagnostics) = FunctionLowerer::new(
            &synthetic,
            &plan.body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        output.diagnostics.extend(diagnostics);''',
)
replace(
    "crates/forge-frontend/src/fir_v1.rs",
    '''        let (function, mut diagnostics) = FunctionLowerer::new(
            body,
            typed_body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        diagnostics.extend(verify_fir_function(&function));
        output.diagnostics.extend(diagnostics);''',
    '''        let (function, diagnostics) = FunctionLowerer::new(
            body,
            typed_body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        output.diagnostics.extend(diagnostics);''',
)

# Public exports.
replace(
    "crates/forge-frontend/src/lib.rs",
    '''pub use fir::{
    lower_fir, verify_fir_function, FirBasicBlock, FirBlockId, FirConst, FirDiagnostic,''',
    '''pub use fir::{
    dump_fir_module, lower_fir, verify_fir_boundary, verify_fir_function, verify_fir_module,
    FirBasicBlock, FirBlockId, FirConst, FirDiagnostic,''',
)

# Focused hardening + golden tests.
p = Path("crates/forge-frontend/tests/fir.rs")
text = p.read_text()
text = text.replace(
    '''    lower_fir, lower_module, lower_resolved_bodies, parse_source, type_check_module,
    FirInstructionKind, FirTerminator, OverflowMode, Ty, TypedExprKind,
''',
    '''    dump_fir_module, lower_fir, lower_module, lower_resolved_bodies, parse_source,
    type_check_module, verify_fir_boundary, verify_fir_module, FirInstructionKind, FirTerminator,
    OverflowMode, Ty, TypedExprKind,
''',
)
text += r'''

#[test]
fn boundary_verifier_rejects_non_concrete_semantic_output() {
    let parsed = parse_source(
        r#"
        module test.boundary_bad_type;
        fn main() -> u32 { return 1u32; }
        "#,
    );
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let mut typed = type_check_module(&ast, &hir.module, &bodies);
    let body = typed.functions.values_mut().next().unwrap();
    body.expressions[0].ty = Ty::Unknown;
    let diagnostics = verify_fir_boundary(&bodies, &typed);
    assert!(diagnostics.iter().any(|d| d.code == "fir/boundary-type"));
}

#[test]
fn duration_is_resolved_before_fir_boundary() {
    let parsed = parse_source(
        r#"
        module test.boundary_duration;
        struct Timer {}
        impl Timer { fn recv(self: &Timer) -> u32 { return 1u32; } }
        fn main(timer: Timer) -> void {
            select {
                recv timer -> _ => { return; }
                timeout #duration "5ms" => { return; }
            }
        }
        "#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    assert!(typed.functions.values().any(|body| body.expressions.iter().any(|expr| matches!(
        expr.kind,
        TypedExprKind::ResolvedDuration { .. }
    ))));
    assert!(verify_fir_boundary(&bodies, &typed).is_empty());
}

#[test]
fn module_verifier_rejects_poison_and_broken_global_init_order() {
    let mut output = lower(
        r#"
        module test.module_verify;
        fn seed() -> u32 { return 1u32; }
        val dependent: u32 = base + 1u32;
        val base: u32 = seed();
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    output.module.global_init_order.reverse();
    let first_function = output.module.functions.values_mut().next().unwrap();
    first_function.blocks[0].instructions[0].kind = FirInstructionKind::Poison;
    let diagnostics = verify_fir_module(&output.module);
    assert!(diagnostics.iter().any(|d| d.code == "fir/verify-poison"));
    assert!(diagnostics
        .iter()
        .any(|d| d.code == "fir/verify-global-init-order"));
}

#[test]
fn fir_dump_golden_is_stable() {
    let output = lower(
        r#"
        module test.fir_dump_golden;
        fn seed() -> u32 { return 2u32; }
        val base: u32 = seed();
        fn add(value: u32) -> u32 { return base + value; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let actual = dump_fir_module(&output.module) + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden/fir_module_v1.json");
    if std::env::var_os("UPDATE_FIR_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).expect("checked-in FIR golden");
    assert_eq!(actual, expected);
}
'''
p.write_text(text)

# Plan completion.
p = Path("docs/fir-completion-plan.md")
text = p.read_text()
text = text.replace(
    "- Step 16 intentionally untouched.",
    "- Step 16 complete: the typed-HIR/FIR boundary and complete FIR module are verified as target-independent contracts; source-only semantic forms and Poison cannot survive successful lowering, runtime-global init topology is re-verified, and deterministic FIR JSON has checked-in golden coverage.",
)
text += r'''

## Step 16 acceptance tests

- Successful typed HIR is checked module-wide before lowering: function/global/initializer types must be concrete and typed expression IDs must be unique within each body.
- Semantic source forms that require prior resolution (`context`, `?`, `match`, closures, reader forms) are rejected at the boundary if still represented as generic `Source` expressions.
- `#duration "..."` is normalized to `TypedExprKind::ResolvedDuration`; FIR no longer interprets reader-form payload semantics itself.
- Resolved semantic expression variants are checked against their expected HIR shapes before FIR consumes them.
- Whole-module FIR verification covers globals, normal functions, runtime initializer functions, concrete value types, block/function validity, and the complete runtime-global initializer order/dependency topology.
- A successfully lowered module may not contain `FirInstructionKind::Poison`.
- `dump_fir_module` emits deterministic pretty JSON and a checked-in golden locks the serialized FIR/module-init representation.
- Existing focused FIR tests for Steps 1-15 remain green alongside the new boundary/golden tests.
'''
p.write_text(text)
