from pathlib import Path


def replace(path, old, new, count=1):
    p = Path(path)
    s = p.read_text()
    if old not in s:
        raise SystemExit(f"anchor not found in {path}: {old[:120]!r}")
    p.write_text(s.replace(old, new, count))

# ---------------------------------------------------------------------------
# typecheck_v1.rs: explicit unsafe scopes, operation plans and provenance
# ---------------------------------------------------------------------------
p = Path('crates/forge-frontend/src/typecheck_v1.rs')
s = p.read_text()

anchor = '''#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedSelectPlan {
    pub span: Span,
    pub operation: RuntimeOperationId,
    pub arms: Vec<TypedSelectArm>,
}

'''
insert = anchor + '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub enum UnsafeOperationKind {
    RawDereference { volatile: bool },
    PointerOffset { subtract: bool },
    PointerToInteger,
    IntegerToPointer,
    PointerReinterpret,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UnsafeProvenance {
    pub scope: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TypedUnsafeScope {
    pub span: Span,
}

'''
if anchor not in s: raise SystemExit('typed select anchor missing')
s = s.replace(anchor, insert, 1)

s = s.replace(
'''    ResolvedMatch {
        plan: TypedMatchPlan,
        hir: HirExpr,
    },
    OptionalPromote {
''',
'''    ResolvedMatch {
        plan: TypedMatchPlan,
        hir: HirExpr,
    },
    UnsafeOperation {
        operation: UnsafeOperationKind,
        provenance: UnsafeProvenance,
        hir: HirExpr,
    },
    OptionalPromote {
''', 1)

s = s.replace(
'''    pub context_scopes: Vec<TypedContextScope>,
    pub select_plans: Vec<TypedSelectPlan>,
    pub expressions: Vec<TypedExpr>,
''',
'''    pub context_scopes: Vec<TypedContextScope>,
    pub select_plans: Vec<TypedSelectPlan>,
    pub unsafe_scopes: Vec<TypedUnsafeScope>,
    pub expressions: Vec<TypedExpr>,
''', 1)

s = s.replace(
'''                context_scopes: checker.context_scopes,
                select_plans: checker.select_plans,
                expressions: checker.expressions,
''',
'''                context_scopes: checker.context_scopes,
                select_plans: checker.select_plans,
                unsafe_scopes: checker.unsafe_scopes,
                expressions: checker.expressions,
''', 1)

s = s.replace(
'''    context_types: BTreeMap<ContextSlot, Ty>,
    context_scopes: Vec<TypedContextScope>,
    select_plans: Vec<TypedSelectPlan>,
    expressions: Vec<TypedExpr>,
''',
'''    context_types: BTreeMap<ContextSlot, Ty>,
    context_scopes: Vec<TypedContextScope>,
    select_plans: Vec<TypedSelectPlan>,
    unsafe_scopes: Vec<TypedUnsafeScope>,
    unsafe_stack: Vec<Span>,
    expressions: Vec<TypedExpr>,
''', 1)

s = s.replace(
'''            context_types: BTreeMap::new(),
            context_scopes: Vec::new(),
            select_plans: Vec::new(),
            expressions: Vec::new(),
''',
'''            context_types: BTreeMap::new(),
            context_scopes: Vec::new(),
            select_plans: Vec::new(),
            unsafe_scopes: Vec::new(),
            unsafe_stack: Vec::new(),
            expressions: Vec::new(),
''', 1)

old_stmt = '''            HirStmtKind::DeferBlock { block }
            | HirStmtKind::Unsafe { block }
            | HirStmtKind::Block { block } => self.check_block(block),
'''
new_stmt = '''            HirStmtKind::DeferBlock { block } | HirStmtKind::Block { block } => {
                self.check_block(block)
            }
            HirStmtKind::Unsafe { block } => {
                self.unsafe_scopes.push(TypedUnsafeScope { span: stmt.span });
                self.unsafe_stack.push(stmt.span);
                self.check_block(block);
                self.unsafe_stack.pop();
            }
'''
if old_stmt not in s: raise SystemExit('unsafe stmt anchor missing')
s = s.replace(old_stmt, new_stmt, 1)

s = s.replace(
'''        let mut resolved_try: Option<(Ty, Ty)> = None;
        let mut resolved_match: Option<TypedMatchPlan> = None;
        let mut ty = match &expr.kind {
''',
'''        let mut resolved_try: Option<(Ty, Ty)> = None;
        let mut resolved_match: Option<TypedMatchPlan> = None;
        let mut resolved_unsafe: Option<(UnsafeOperationKind, UnsafeProvenance)> = None;
        let mut ty = match &expr.kind {
''', 1)

old_unary = '''            HirExprKind::Unary { op, value } => {
                let v = self.check_expr(value, expected);
                self.check_unary(expr.span, *op, v)
            }
            HirExprKind::Binary { op, left, right } => {
                self.check_binary(expr.span, *op, left, right, expected)
            }
'''
new_unary = '''            HirExprKind::Unary { op, value } => {
                let v = self.check_expr(value, expected);
                let result = self.check_unary(expr.span, *op, v.clone());
                if *op == UnaryOp::Deref {
                    if let Ty::Pointer { volatile, .. } = v {
                        let operation = UnsafeOperationKind::RawDereference { volatile };
                        if let Some(provenance) = self.authorize_unsafe(expr.span, operation) {
                            resolved_unsafe = Some((operation, provenance));
                        }
                    }
                }
                result
            }
            HirExprKind::Binary { op, left, right } => self.check_binary(
                expr.span,
                *op,
                left,
                right,
                expected,
                &mut resolved_unsafe,
            ),
'''
if old_unary not in s: raise SystemExit('unary/binary check anchor missing')
s = s.replace(old_unary, new_unary, 1)

s = s.replace(
'''            HirExprKind::TypeCall { target, args } => self.check_type_call(expr.span, target, args),
''',
'''            HirExprKind::TypeCall { target, args } => {
                self.check_type_call(expr.span, target, args, &mut resolved_unsafe)
            }
''', 1)

old_base = '''        let base_kind = if let Some((source_error, target_error)) = resolved_try {
            TypedExprKind::ResolvedTry {
'''
new_base = '''        let base_kind = if let Some((operation, provenance)) = resolved_unsafe {
            TypedExprKind::UnsafeOperation {
                operation,
                provenance,
                hir: expr.clone(),
            }
        } else if let Some((source_error, target_error)) = resolved_try {
            TypedExprKind::ResolvedTry {
'''
if old_base not in s: raise SystemExit('base kind anchor missing')
s = s.replace(old_base, new_base, 1)

# Add unsafe authorization helper before is_type_expr.
helper_anchor = '''    fn is_type_expr(&self, expr: &HirExpr) -> bool {
'''
helper = '''    fn authorize_unsafe(
        &mut self,
        span: Span,
        operation: UnsafeOperationKind,
    ) -> Option<UnsafeProvenance> {
        if let Some(scope) = self.unsafe_stack.last().copied() {
            Some(UnsafeProvenance { scope })
        } else {
            self.diagnostic(
                span,
                "unsafe/required",
                format!("{operation:?} requires an enclosing `unsafe` block"),
            );
            None
        }
    }

    fn is_type_expr(&self, expr: &HirExpr) -> bool {
'''
if helper_anchor not in s: raise SystemExit('is_type_expr anchor missing')
s = s.replace(helper_anchor, helper, 1)

# Replace check_type_call with an unsafe-aware version.
start = s.index('    fn check_type_call(')
end = s.index('\n    fn check_binary(', start)
old = s[start:end]
new = '''    fn check_type_call(
        &mut self,
        span: Span,
        target: &HirTypeRef,
        args: &[HirCallArg],
        resolved_unsafe: &mut Option<(UnsafeOperationKind, UnsafeProvenance)>,
    ) -> Ty {
        let target_ty = self.env.ty_from_ref(target);
        if args.len() != 1 {
            self.diagnostic(
                span,
                "call/arity",
                "type conversion/construction requires exactly one argument",
            );
            return Ty::Error;
        }
        let source = self.check_expr(arg_value(&args[0]), None);

        let unsafe_conversion = match (&source, &target_ty) {
            (Ty::Pointer { .. }, Ty::Int { .. } | Ty::Byte) => {
                Some(UnsafeOperationKind::PointerToInteger)
            }
            (Ty::Int { .. } | Ty::Byte, Ty::Pointer { .. }) => {
                Some(UnsafeOperationKind::IntegerToPointer)
            }
            (Ty::Pointer { .. }, Ty::Pointer { .. }) if source != target_ty => {
                Some(UnsafeOperationKind::PointerReinterpret)
            }
            _ => None,
        };
        if let Some(operation) = unsafe_conversion {
            if let Some(provenance) = self.authorize_unsafe(span, operation) {
                *resolved_unsafe = Some((operation, provenance));
            }
            return target_ty;
        }

        match &target_ty {
            Ty::Nominal(id) => {
                if let Some(underlying) = self.env.distinct_underlying(*id) {
                    if !self.is_explicitly_convertible(underlying, &source) {
                        self.diagnostic(
                            span,
                            "type/distinct",
                            format!("cannot construct distinct type from {source:?}"),
                        );
                    }
                }
            }
            Ty::Int { .. } | Ty::Float { .. } | Ty::Byte | Ty::Char => {
                if !self.is_explicitly_convertible(&target_ty, &source) {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("invalid explicit conversion from {source:?} to {target_ty:?}"),
                    );
                }
            }
            Ty::Pointer { .. } => {
                if source != target_ty {
                    self.diagnostic(
                        span,
                        "type/mismatch",
                        format!("invalid explicit conversion from {source:?} to {target_ty:?}"),
                    );
                }
            }
            _ => {}
        }
        target_ty
    }
'''
s = s[:start] + new + s[end:]

# Replace check_binary signature/body prefix with pointer arithmetic handling.
old_sig = '''    fn check_binary(
        &mut self,
        span: Span,
        op: BinaryOp,
        left: &HirExpr,
        right: &HirExpr,
        expected: Option<&Ty>,
    ) -> Ty {
        let l = self.check_expr(left, expected);
        let r = self.check_expr(
            right,
            if is_concrete_numeric(&l) {
                Some(&l)
            } else {
                expected
            },
        );
        match op {
'''
new_sig = '''    fn check_binary(
        &mut self,
        span: Span,
        op: BinaryOp,
        left: &HirExpr,
        right: &HirExpr,
        expected: Option<&Ty>,
        resolved_unsafe: &mut Option<(UnsafeOperationKind, UnsafeProvenance)>,
    ) -> Ty {
        let l = self.check_expr(left, expected);
        let r = self.check_expr(
            right,
            if is_concrete_numeric(&l) {
                Some(&l)
            } else {
                expected
            },
        );
        if matches!(op, BinaryOp::Add | BinaryOp::Sub)
            && matches!(l, Ty::Pointer { .. })
            && is_integer_like(&r)
        {
            let operation = UnsafeOperationKind::PointerOffset {
                subtract: op == BinaryOp::Sub,
            };
            if let Some(provenance) = self.authorize_unsafe(span, operation) {
                *resolved_unsafe = Some((operation, provenance));
            }
            return l;
        }
        match op {
'''
if old_sig not in s: raise SystemExit('check_binary signature anchor missing')
s = s.replace(old_sig, new_sig, 1)

p.write_text(s)

# ---------------------------------------------------------------------------
# fir_v1.rs: dedicated raw operations carrying the semantic provenance
# ---------------------------------------------------------------------------
p = Path('crates/forge-frontend/src/fir_v1.rs')
s = p.read_text()
s = s.replace(
'''        TypeCheckOutput, TypedBody, TypedClosurePlan, TypedExpr, TypedExprKind, TypedMatchBinding,
        TypedMatchPlan, TypedSelectArm,
''',
'''        TypeCheckOutput, TypedBody, TypedClosurePlan, TypedExpr, TypedExprKind, TypedMatchBinding,
        TypedMatchPlan, TypedSelectArm, UnsafeOperationKind, UnsafeProvenance,
''', 1)

s = s.replace(
'''    Convert {
        value: FirValueId,
        target: Ty,
    },
    MakeArray {
''',
'''    Convert {
        value: FirValueId,
        target: Ty,
    },
    PointerOffset {
        pointer: FirValueId,
        offset: FirValueId,
        subtract: bool,
        provenance: UnsafeProvenance,
    },
    PointerConvert {
        value: FirValueId,
        target: Ty,
        operation: UnsafeOperationKind,
        provenance: UnsafeProvenance,
    },
    MakeArray {
''', 1)

s = s.replace(
'''    Deref {
        address: FirValueId,
    },
}
''',
'''    Deref {
        address: FirValueId,
    },
    RawDeref {
        address: FirValueId,
        volatile: bool,
        provenance: UnsafeProvenance,
    },
}
''', 1)

s = s.replace(
'''            TypedExprKind::ResolvedMatch { plan, .. } => self.lower_match(expr, plan, result_ty),
            TypedExprKind::Source { .. } => self.lower_source_expr(expr, result_ty),
''',
'''            TypedExprKind::ResolvedMatch { plan, .. } => self.lower_match(expr, plan, result_ty),
            TypedExprKind::UnsafeOperation {
                operation,
                provenance,
                ..
            } => self.lower_unsafe_expr(expr, *operation, *provenance, result_ty),
            TypedExprKind::Source { .. } => self.lower_source_expr(expr, result_ty),
''', 1)

# Insert unsafe lowering before lower_closure.
anchor = '''    fn lower_closure(
        &mut self,
        expr: &HirExpr,
'''
helper = '''    fn lower_unsafe_expr(
        &mut self,
        expr: &HirExpr,
        operation: UnsafeOperationKind,
        provenance: UnsafeProvenance,
        result_ty: Ty,
    ) -> FirValueId {
        if !self
            .typed
            .unsafe_scopes
            .iter()
            .any(|scope| scope.span == provenance.scope)
            || provenance.scope.start > expr.span.start
            || provenance.scope.end < expr.span.end
        {
            self.diagnostic(
                expr.span,
                "fir/unsafe-provenance",
                "unsafe operation has no valid enclosing semantic authorization scope",
            );
            return self.poison(expr.span, result_ty);
        }

        match (operation, &expr.kind) {
            (
                UnsafeOperationKind::RawDereference { volatile },
                HirExprKind::Unary {
                    op: UnaryOp::Deref,
                    value,
                },
            ) => {
                let address = self.lower_expr(value);
                self.emit_value(
                    expr.span,
                    result_ty,
                    FirInstructionKind::Load {
                        place: FirPlace::RawDeref {
                            address,
                            volatile,
                            provenance,
                        },
                    },
                )
            }
            (
                UnsafeOperationKind::PointerOffset { subtract },
                HirExprKind::Binary {
                    op: BinaryOp::Add | BinaryOp::Sub,
                    left,
                    right,
                },
            ) => {
                let pointer = self.lower_expr(left);
                let offset = self.lower_expr(right);
                self.emit_value(
                    expr.span,
                    result_ty,
                    FirInstructionKind::PointerOffset {
                        pointer,
                        offset,
                        subtract,
                        provenance,
                    },
                )
            }
            (
                operation @ (UnsafeOperationKind::PointerToInteger
                | UnsafeOperationKind::IntegerToPointer
                | UnsafeOperationKind::PointerReinterpret),
                HirExprKind::TypeCall { args, .. },
            ) => {
                let Some(value) = first_positional(args) else {
                    self.diagnostic(
                        expr.span,
                        "fir/unsafe-conversion-shape",
                        "unsafe pointer conversion has no positional operand",
                    );
                    return self.poison(expr.span, result_ty);
                };
                let value = self.lower_expr(value);
                self.emit_value(
                    expr.span,
                    result_ty.clone(),
                    FirInstructionKind::PointerConvert {
                        value,
                        target: result_ty,
                        operation,
                        provenance,
                    },
                )
            }
            _ => {
                self.diagnostic(
                    expr.span,
                    "fir/unsafe-plan-shape",
                    "typed unsafe operation does not match its source expression",
                );
                self.poison(expr.span, result_ty)
            }
        }
    }

    fn lower_closure(
        &mut self,
        expr: &HirExpr,
'''
if anchor not in s: raise SystemExit('lower_closure anchor missing')
s = s.replace(anchor, helper, 1)

# Raw pointer source expressions must never bypass the semantic plan.
old_binary = '''            HirExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
'''
new_binary = '''            HirExprKind::Binary { op, left, right } => {
                let left_ty = self.expr_ty(left);
                if matches!(left_ty, Ty::Pointer { .. })
                    && matches!(op, BinaryOp::Add | BinaryOp::Sub)
                {
                    self.diagnostic(
                        expr.span,
                        "fir/unsafe-authorization-missing",
                        "raw pointer arithmetic reached FIR without semantic unsafe provenance",
                    );
                    return self.poison(expr.span, ty);
                }
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
'''
if old_binary not in s: raise SystemExit('source binary anchor missing')
s = s.replace(old_binary, new_binary, 1)

old_typecall = '''            HirExprKind::TypeCall { args, .. } => {
                let Some(value) = first_positional(args) else {
'''
new_typecall = '''            HirExprKind::TypeCall { args, .. } => {
                let Some(source_expr) = first_positional(args) else {
                    self.diagnostic(
                        expr.span,
                        "fir/conversion-arity",
                        "type conversion requires one positional operand",
                    );
                    return self.poison(expr.span, ty);
                };
                let source_ty = self.expr_ty(source_expr);
                let pointer_conversion = matches!(source_ty, Ty::Pointer { .. })
                    && matches!(ty, Ty::Pointer { .. } | Ty::Int { .. } | Ty::Byte)
                    || matches!(ty, Ty::Pointer { .. })
                        && matches!(source_ty, Ty::Int { .. } | Ty::Byte);
                if pointer_conversion {
                    self.diagnostic(
                        expr.span,
                        "fir/unsafe-authorization-missing",
                        "raw pointer conversion reached FIR without semantic unsafe provenance",
                    );
                    return self.poison(expr.span, ty);
                }
                let Some(value) = first_positional(args) else {
'''
if old_typecall not in s: raise SystemExit('source typecall anchor missing')
s = s.replace(old_typecall, new_typecall, 1)

# Raw pointer dereference via Source is forbidden; references remain ordinary Deref places.
old_deref = '''            UnaryOp::Deref => {
                let address = self.lower_expr(value);
                self.emit_value(
                    span,
                    ty,
                    FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                )
            }
'''
new_deref = '''            UnaryOp::Deref => {
                if matches!(self.expr_ty(value), Ty::Pointer { .. }) {
                    self.diagnostic(
                        span,
                        "fir/unsafe-authorization-missing",
                        "raw pointer dereference reached FIR without semantic unsafe provenance",
                    );
                    return self.poison(span, ty);
                }
                let address = self.lower_expr(value);
                self.emit_value(
                    span,
                    ty,
                    FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                )
            }
'''
if old_deref not in s: raise SystemExit('lower_unary deref anchor missing')
s = s.replace(old_deref, new_deref, 1)

old_place = '''            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => Some(FirPlace::Deref {
                address: self.lower_expr(value),
            }),
'''
new_place = '''            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => {
                let value_ty = self.expr_ty(value);
                let address = self.lower_expr(value);
                if let Ty::Pointer { volatile, .. } = value_ty {
                    let typed = self.typed_expr(expr)?.clone();
                    let TypedExprKind::UnsafeOperation {
                        operation: UnsafeOperationKind::RawDereference { volatile: planned_volatile },
                        provenance,
                        ..
                    } = typed.kind
                    else {
                        self.diagnostic(
                            expr.span,
                            "fir/unsafe-authorization-missing",
                            "raw pointer place reached FIR without semantic unsafe provenance",
                        );
                        return None;
                    };
                    if planned_volatile != volatile
                        || !self
                            .typed
                            .unsafe_scopes
                            .iter()
                            .any(|scope| scope.span == provenance.scope)
                    {
                        self.diagnostic(
                            expr.span,
                            "fir/unsafe-provenance",
                            "raw pointer place has invalid unsafe provenance",
                        );
                        return None;
                    }
                    Some(FirPlace::RawDeref {
                        address,
                        volatile,
                        provenance,
                    })
                } else {
                    Some(FirPlace::Deref { address })
                }
            }
'''
if old_place not in s: raise SystemExit('try_place deref anchor missing')
s = s.replace(old_place, new_place, 1)

p.write_text(s)

# ---------------------------------------------------------------------------
# Public surface
# ---------------------------------------------------------------------------
p = Path('crates/forge-frontend/src/lib.rs')
s = p.read_text()
s = s.replace(
'''    TypedMatchBinding, TypedMatchPlan,
};
''',
'''    TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,
};
''', 1)
p.write_text(s)

# ---------------------------------------------------------------------------
# Step 12 focused typechecker tests
# ---------------------------------------------------------------------------
p = Path('crates/forge-frontend/tests/typecheck.rs')
s = p.read_text()
s += r'''

#[test]
fn raw_pointer_dereference_requires_unsafe_authorization() {
    let output = check(
        r#"
        module test.raw_deref_safe;
        fn read(p: *u32) -> u32 { return *p; }
        "#,
    );
    assert!(has(&output, "unsafe/required"), "{:?}", output.diagnostics);
}

#[test]
fn unsafe_raw_pointer_dereference_records_scope_provenance() {
    let output = check(
        r#"
        module test.raw_deref_unsafe;
        fn read(p: *u32) -> u32 {
            unsafe { return *p; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert_eq!(body.unsafe_scopes.len(), 1);
    let raw = body.expressions.iter().find_map(|expr| match &expr.kind {
        forge_frontend::TypedExprKind::UnsafeOperation {
            operation: forge_frontend::UnsafeOperationKind::RawDereference { volatile: false },
            provenance,
            ..
        } => Some((expr.span, *provenance)),
        _ => None,
    }).expect("typed raw dereference");
    assert_eq!(raw.1.scope, body.unsafe_scopes[0].span);
    assert!(raw.1.scope.start <= raw.0.start && raw.1.scope.end >= raw.0.end);
}

#[test]
fn safe_reference_dereference_needs_no_unsafe_provenance() {
    let output = check(
        r#"
        module test.reference_deref;
        fn read(p: &u32) -> u32 { return *p; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.unsafe_scopes.is_empty());
    assert!(!body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation { .. }
    )));
}

#[test]
fn raw_pointer_arithmetic_requires_and_records_unsafe() {
    let safe = check(
        r#"
        module test.pointer_offset_safe;
        fn next(p: *u32) -> *u32 { return p + 1usize; }
        "#,
    );
    assert!(has(&safe, "unsafe/required"), "{:?}", safe.diagnostics);

    let unsafe_output = check(
        r#"
        module test.pointer_offset_unsafe;
        fn next(p: *u32) -> *u32 {
            unsafe { return p + 1usize; }
        }
        "#,
    );
    assert!(unsafe_output.diagnostics.is_empty(), "{:?}", unsafe_output.diagnostics);
    let body = unsafe_output.functions.values().next().unwrap();
    assert!(body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation {
            operation: forge_frontend::UnsafeOperationKind::PointerOffset { subtract: false },
            ..
        }
    )));
}

#[test]
fn pointer_integer_and_reinterpret_conversions_require_unsafe() {
    let safe = check(
        r#"
        module test.pointer_cast_safe;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { return usize(p); }
        fn cast(p: *u32) -> BytePtr { return BytePtr(p); }
        "#,
    );
    assert!(safe.diagnostics.iter().filter(|d| d.code == "unsafe/required").count() >= 2,
        "{:?}", safe.diagnostics);

    let unsafe_output = check(
        r#"
        module test.pointer_cast_unsafe;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { unsafe { return usize(p); } }
        fn cast(p: *u32) -> BytePtr { unsafe { return BytePtr(p); } }
        "#,
    );
    assert!(unsafe_output.diagnostics.is_empty(), "{:?}", unsafe_output.diagnostics);
    let kinds = unsafe_output.functions.values().flat_map(|body| body.expressions.iter()).filter_map(|expr| {
        match &expr.kind {
            forge_frontend::TypedExprKind::UnsafeOperation { operation, .. } => Some(*operation),
            _ => None,
        }
    }).collect::<Vec<_>>();
    assert!(kinds.contains(&forge_frontend::UnsafeOperationKind::PointerToInteger));
    assert!(kinds.contains(&forge_frontend::UnsafeOperationKind::PointerReinterpret));
}

#[test]
fn integer_to_pointer_alias_conversion_requires_unsafe() {
    let output = check(
        r#"
        module test.address_to_pointer;
        type Raw = *u32;
        fn from_address(address: usize) -> Raw {
            unsafe { return Raw(address); }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::UnsafeOperation {
            operation: forge_frontend::UnsafeOperationKind::IntegerToPointer,
            ..
        }
    )));
}
'''
p.write_text(s)

# ---------------------------------------------------------------------------
# Step 12 focused FIR tests
# ---------------------------------------------------------------------------
p = Path('crates/forge-frontend/tests/fir.rs')
s = p.read_text()
s += r'''

#[test]
fn raw_pointer_dereference_lowers_with_unsafe_provenance() {
    let output = lower(
        r#"
        module test.fir_raw_deref;
        fn read(p: *u32) -> u32 {
            unsafe { return *p; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::RawDeref {
                volatile: false,
                ..
            }
        }
    )));
}

#[test]
fn raw_pointer_store_uses_provenanced_raw_place() {
    let output = lower(
        r#"
        module test.fir_raw_store;
        fn write(p: *u32, value: u32) {
            unsafe { *p = value; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Store {
            place: forge_frontend::FirPlace::RawDeref { .. },
            ..
        }
    )));
}

#[test]
fn pointer_offset_is_a_dedicated_provenanced_fir_operation() {
    let output = lower(
        r#"
        module test.fir_pointer_offset;
        fn previous(p: *u32) -> *u32 {
            unsafe { return p - 2usize; }
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerOffset { subtract: true, .. }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Binary {
            op: forge_frontend::ast::BinaryOp::Sub,
            ..
        }
    )));
}

#[test]
fn pointer_conversions_are_not_plain_fir_converts() {
    let output = lower(
        r#"
        module test.fir_pointer_convert;
        type BytePtr = *byte;
        fn address(p: *u32) -> usize { unsafe { return usize(p); } }
        fn cast(p: *u32) -> BytePtr { unsafe { return BytePtr(p); } }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerConvert {
            operation: forge_frontend::UnsafeOperationKind::PointerToInteger,
            ..
        }
    )));
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::PointerConvert {
            operation: forge_frontend::UnsafeOperationKind::PointerReinterpret,
            ..
        }
    )));
}

#[test]
fn ordinary_reference_deref_remains_safe_fir_deref() {
    let output = lower(
        r#"
        module test.fir_reference_deref;
        fn read(p: &u32) -> u32 { return *p; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::Deref { .. }
        }
    )));
    assert!(!instructions(&output).any(|instruction| matches!(
        instruction,
        FirInstructionKind::Load {
            place: forge_frontend::FirPlace::RawDeref { .. }
        }
    )));
}
'''
p.write_text(s)

# ---------------------------------------------------------------------------
# Plan completion record + acceptance tests
# ---------------------------------------------------------------------------
p = Path('docs/fir-completion-plan.md')
s = p.read_text()
s = s.replace(
'- Steps 12-16 intentionally untouched.',
'- Step 12 complete: unsafe scopes are semantic authorization scopes; raw dereference, pointer arithmetic, and pointer/integer or reinterpret conversions carry explicit source-scope provenance into dedicated FIR raw operations.\n- Steps 13-16 intentionally untouched.',
1)
s += '''\n\n## Step 12 acceptance tests\n\n- Raw pointer dereference is rejected with `unsafe/required` outside an `unsafe` block; safe-reference dereference remains ordinary safe code.\n- Every accepted raw operation carries `UnsafeProvenance` naming the exact enclosing semantic unsafe scope.\n- Raw pointer `+`/`-` integer arithmetic is accepted only with unsafe authorization and lowers to dedicated `PointerOffset` FIR rather than ordinary numeric `Binary`.\n- Pointer-to-integer, integer-to-pointer, and pointer reinterpret conversions require unsafe authorization and lower to dedicated `PointerConvert` FIR rather than ordinary `Convert`.\n- Raw pointer loads/stores use `FirPlace::RawDeref` with provenance; reference loads/stores continue to use `FirPlace::Deref`.\n- FIR validates that provenance refers to a typed unsafe scope enclosing the source operation before emitting a raw operation.\n- Entering `unsafe` does not disable array/slice bounds checks, checked arithmetic, or ordinary type checking.\n'''
p.write_text(s)
