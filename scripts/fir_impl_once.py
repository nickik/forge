from pathlib import Path


def replace_once(path: str, old: str, new: str) -> None:
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"missing replacement in {path}: {old[:160]!r}")
    p.write_text(text.replace(old, new, 1))


# ---------------------------------------------------------------------------
# Give expressions stable body-local identity. FIR must never recover typed
# semantics by matching source spans or cloned HIR syntax.
# ---------------------------------------------------------------------------
p = Path("crates/forge-frontend/src/body_hir_v1.rs")
text = p.read_text()
text = text.replace(
    "pub type HirExpr = HirNode<HirExprKind>;\npub type HirStmt = HirNode<HirStmtKind>;",
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct ExprId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirExpr {
    pub id: ExprId,
    pub span: Span,
    pub kind: HirExprKind,
}

impl HirExpr {
    fn new(id: ExprId, span: Span, kind: HirExprKind) -> Self {
        Self { id, span, kind }
    }
}

pub type HirStmt = HirNode<HirStmtKind>;''',
    1,
)
text = text.replace(
    '''    locals: Vec<HirLocalDecl>,
    next_local: u32,
}''',
    '''    locals: Vec<HirLocalDecl>,
    next_local: u32,
    next_expr: u32,
}''',
    1,
)
text = text.replace(
    '''            locals: Vec::new(),
            next_local: 0,
        }''',
    '''            locals: Vec::new(),
            next_local: 0,
            next_expr: 0,
        }''',
    1,
)
old = '''        HirNode::new(expr.span, kind)
    }

    fn lower_binding_pattern'''
new = '''        let id = ExprId(self.next_expr);
        self.next_expr += 1;
        HirExpr::new(id, expr.span, kind)
    }

    fn lower_binding_pattern'''
if old not in text:
    raise SystemExit("lower_expr terminator not found")
text = text.replace(old, new, 1)
p.write_text(text)


# ---------------------------------------------------------------------------
# Typed HIR: carry expression identity, exact function signature, receiver
# transformation, named-argument parameter mapping, and real optional promote.
# ---------------------------------------------------------------------------
p = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = p.read_text()
text = text.replace(
    '''        BodyHirOutput, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern, HirPatternKind,
        HirStmt, HirStmtKind, HirType, HirTypeKind, HirTypeRef,
''',
    '''        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirPattern,
        HirPatternKind, HirStmt, HirStmtKind, HirType, HirTypeKind, HirTypeRef,
''',
    1,
)
text = text.replace(
    '''pub struct TypedExpr {
    pub span: Span,
    pub ty: Ty,
    pub kind: TypedExprKind,
}
''',
    '''pub struct TypedExpr {
    pub id: ExprId,
    pub span: Span,
    pub ty: Ty,
    pub kind: TypedExprKind,
}
''',
    1,
)
text = text.replace(
    '''    ResolvedCall {
        target: DefId,
        method: bool,
        hir: HirExpr,
    },''',
    '''    ResolvedCall {
        target: DefId,
        method: bool,
        receiver: Option<ResolvedReceiver>,
        argument_parameters: Vec<usize>,
        hir: HirExpr,
    },''',
    1,
)
text = text.replace(
    '''    OptionalPromote {
        value: Box<TypedExpr>,
    },
}''',
    '''    OptionalPromote {
        source_type: Ty,
        inner: Box<TypedExprKind>,
        hir: HirExpr,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedReceiver {
    Value,
    SharedReference,
    MutableReference,
}''',
    1,
)
text = text.replace(
    '''pub struct TypedBody {
    pub owner: DefId,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
}''',
    '''pub struct TypedBody {
    pub owner: DefId,
    pub params: Vec<(LocalId, Ty)>,
    pub return_type: Ty,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
}''',
    1,
)

# Function signature retention.
text = text.replace(
    '''        let mut checker = BodyChecker::new(&env, expected_return, &mut output.diagnostics);
        for (local, ty) in &body.params {''',
    '''        let mut checker =
            BodyChecker::new(&env, expected_return.clone(), &mut output.diagnostics);
        let mut typed_params = Vec::with_capacity(body.params.len());
        for (local, ty) in &body.params {''',
    1,
)
text = text.replace(
    '''            checker.local_types.insert(*local, param_ty);
        }
        checker.check_block(&body.block);''',
    '''            checker.local_types.insert(*local, param_ty.clone());
            typed_params.push((*local, param_ty));
        }
        checker.check_block(&body.block);''',
    1,
)
text = text.replace(
    '''            TypedBody {
                owner: *owner,
                local_types: checker.local_types,
                local_constants: checker.local_constants,
                expressions: checker.expressions,
            },''',
    '''            TypedBody {
                owner: *owner,
                params: typed_params,
                return_type: expected_return,
                local_types: checker.local_types,
                local_constants: checker.local_constants,
                expressions: checker.expressions,
            },''',
    1,
)

# Replace the lightweight call resolution tuple with semantic call facts.
text = text.replace(
    '''        let mut resolved_call: Option<(DefId, bool)> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;''',
    '''        let mut resolved_call: Option<ResolvedCallInfo> = None;
        let mut resolved_try: Option<(Ty, Ty)> = None;''',
    1,
)
text = text.replace(
    '''                let (result, target) = self.check_call(expr.span, callee, args);
                resolved_call = target;
                result''',
    '''                let (result, call) = self.check_call(expr.span, callee, args);
                resolved_call = call;
                result''',
    1,
)

# Replace final contextual-coercion/typed-expression recording block.
old = '''        if let Some(expected) = expected {
            if matches!(ty, Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral)
                && self.is_assignable(expected, &ty)
            {
                ty = expected.clone();
            }
        }
        let kind = if let Some((source_error, target_error)) = resolved_try {
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                hir: expr.clone(),
            }
        } else if let Some((target, method)) = resolved_call {
            TypedExprKind::ResolvedCall {
                target,
                method,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
        self.expressions.push(TypedExpr {
            span: expr.span,
            ty: ty.clone(),
            kind,
        });
        ty
'''
new = '''        let mut optional_promotion = None;
        if let Some(expected) = expected {
            if let Ty::Optional { inner } = expected {
                if matches!(ty, Ty::NoneLiteral) {
                    ty = expected.clone();
                } else if !matches!(ty, Ty::Optional { .. }) && self.is_assignable(inner, &ty) {
                    let source_type = match &ty {
                        Ty::IntLiteral | Ty::FloatLiteral => inner.as_ref().clone(),
                        other => other.clone(),
                    };
                    optional_promotion = Some(source_type);
                    ty = expected.clone();
                }
            } else if matches!(ty, Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral)
                && self.is_assignable(expected, &ty)
            {
                ty = expected.clone();
            }
        }
        let base_kind = if let Some((source_error, target_error)) = resolved_try {
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                hir: expr.clone(),
            }
        } else if let Some(call) = resolved_call {
            TypedExprKind::ResolvedCall {
                target: call.target,
                method: call.method,
                receiver: call.receiver,
                argument_parameters: call.argument_parameters,
                hir: expr.clone(),
            }
        } else {
            TypedExprKind::Source { hir: expr.clone() }
        };
        let kind = if let Some(source_type) = optional_promotion {
            TypedExprKind::OptionalPromote {
                source_type,
                inner: Box::new(base_kind),
                hir: expr.clone(),
            }
        } else {
            base_kind
        };
        self.expressions.push(TypedExpr {
            id: expr.id,
            span: expr.span,
            ty: ty.clone(),
            kind,
        });
        ty
'''
if old not in text:
    raise SystemExit("typed expression finalization block not found")
text = text.replace(old, new, 1)

# Internal call resolution record, immediately before BodyChecker.
marker = "struct BodyChecker<'a, 'd> {"
insert = '''#[derive(Debug, Clone)]
struct ResolvedCallInfo {
    target: DefId,
    method: bool,
    receiver: Option<ResolvedReceiver>,
    argument_parameters: Vec<usize>,
}

'''
if marker not in text:
    raise SystemExit("BodyChecker marker missing")
text = text.replace(marker, insert + marker, 1)

# check_call signature + returns and argument mapping.
text = text.replace(
    ''') -> (Ty, Option<(DefId, bool)>) {''',
    ''') -> (Ty, Option<ResolvedCallInfo>) {''',
    1,
)
old = '''                self.check_method_receiver(base, &receiver_ty, &sig);
                let reduced = FunctionSig {
                    params: sig.params.iter().skip(1).cloned().collect(),
                    result: sig.result.clone(),
                    named_arguments: sig.named_arguments,
                };
                self.check_function_args(span, &reduced, args);
                return (sig.result, Some((method_id, true)));
'''
new = '''                self.check_method_receiver(base, &receiver_ty, &sig);
                let receiver = match sig.params.first().map(|param| &param.ty) {
                    Some(Ty::Reference { mutable: true, .. }) => {
                        Some(ResolvedReceiver::MutableReference)
                    }
                    Some(Ty::Reference { mutable: false, .. }) => {
                        Some(ResolvedReceiver::SharedReference)
                    }
                    Some(_) => Some(ResolvedReceiver::Value),
                    None => None,
                };
                let reduced = FunctionSig {
                    params: sig.params.iter().skip(1).cloned().collect(),
                    result: sig.result.clone(),
                    named_arguments: sig.named_arguments,
                };
                let argument_parameters = self.check_function_args(span, &reduced, args);
                return (
                    sig.result,
                    Some(ResolvedCallInfo {
                        target: method_id,
                        method: true,
                        receiver,
                        argument_parameters,
                    }),
                );
'''
if old not in text:
    raise SystemExit("method call resolution block missing")
text = text.replace(old, new, 1)
old = '''                if let Some(sig) = self.env.functions.get(&id).cloned() {
                    self.check_function_args(span, &sig, args);
                    return (sig.result, Some((id, false)));
                }
'''
new = '''                if let Some(sig) = self.env.functions.get(&id).cloned() {
                    let argument_parameters = self.check_function_args(span, &sig, args);
                    return (
                        sig.result,
                        Some(ResolvedCallInfo {
                            target: id,
                            method: false,
                            receiver: None,
                            argument_parameters,
                        }),
                    );
                }
'''
if old not in text:
    raise SystemExit("direct function call resolution block missing")
text = text.replace(old, new, 1)

# check_function_args now returns semantic parameter index per written argument.
text = text.replace(
    '''    fn check_function_args(&mut self, span: Span, sig: &FunctionSig, args: &[HirCallArg]) {''',
    '''    fn check_function_args(
        &mut self,
        span: Span,
        sig: &FunctionSig,
        args: &[HirCallArg],
    ) -> Vec<usize> {''',
    1,
)
# The named-arguments early error previously returned unit.
text = text.replace(
    '''                return;
            }
            let mut seen = BTreeSet::new();''',
    '''                return Vec::new();
            }
            let mut seen = BTreeSet::new();
            let mut argument_parameters = Vec::with_capacity(args.len());''',
    1,
)
# Record named index when found.
text = text.replace(
    '''                if let Some(param) = sig.params.iter().find(|p| p.name == *name) {
                    let actual = self.check_expr(value, Some(&param.ty));''',
    '''                if let Some((parameter, param)) = sig
                    .params
                    .iter()
                    .enumerate()
                    .find(|(_, p)| p.name == *name)
                {
                    argument_parameters.push(parameter);
                    let actual = self.check_expr(value, Some(&param.ty));''',
    1,
)
# Return named mapping before positional else.
text = text.replace(
    '''            for param in &sig.params {
                if !param.has_default && !seen.contains(&param.name) {
                    self.diagnostic(
                        span,
                        "call/missing-argument",
                        format!("missing required argument `{}`", param.name),
                    );
                }
            }
        } else {
''',
    '''            for param in &sig.params {
                if !param.has_default && !seen.contains(&param.name) {
                    self.diagnostic(
                        span,
                        "call/missing-argument",
                        format!("missing required argument `{}`", param.name),
                    );
                }
            }
            argument_parameters
        } else {
''',
    1,
)
# Positional arm needs final mapping. Locate end of function by a characteristic tail.
old = '''            for param in sig.params.iter().skip(args.len()) {
                if !param.has_default {
                    self.diagnostic(
                        span,
                        "call/missing-argument",
                        format!("missing required argument `{}`", param.name),
                    );
                }
            }
        }
    }
'''
new = '''            for param in sig.params.iter().skip(args.len()) {
                if !param.has_default {
                    self.diagnostic(
                        span,
                        "call/missing-argument",
                        format!("missing required argument `{}`", param.name),
                    );
                }
            }
            (0..args.len().min(sig.params.len())).collect()
        }
    }
'''
if old not in text:
    raise SystemExit("check_function_args tail missing")
text = text.replace(old, new, 1)
p.write_text(text)


# ---------------------------------------------------------------------------
# FIR implementation.
# ---------------------------------------------------------------------------
fir = r'''use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{BinaryOp, FdnValue, MetadataArg, Span, UnaryOp},
    body_hir::{
        BodyHirOutput, ExprId, HirBlock, HirCallArg, HirExpr, HirExprKind, HirMatchBody,
        HirPattern, HirPatternKind, HirStmt, HirStmtKind,
    },
    hir::{DefId, MetadataTableExt, MetadataTarget},
    resolution::{LocalId, ResolvedName},
    typecheck::{
        ConstValue, IntWidth, ResolvedReceiver, Ty, TypeCheckOutput, TypedBody, TypedExpr,
        TypedExprKind,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FirValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FirBlockId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct FirLocalId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowMode {
    Checked,
    Wrapping,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirDiagnostic {
    pub span: Span,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct FirOutput {
    pub module: FirModule,
    pub diagnostics: Vec<FirDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct FirModule {
    pub functions: BTreeMap<DefId, FirFunction>,
    pub globals: BTreeMap<DefId, FirGlobal>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirGlobal {
    pub owner: DefId,
    pub ty: Ty,
    pub constant: Option<ConstValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirFunction {
    pub owner: DefId,
    pub params: Vec<FirLocalId>,
    pub return_type: Ty,
    pub locals: BTreeMap<FirLocalId, FirLocal>,
    pub entry: FirBlockId,
    pub blocks: Vec<FirBasicBlock>,
    pub value_types: BTreeMap<FirValueId, Ty>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirLocal {
    pub id: FirLocalId,
    pub source: Option<LocalId>,
    pub ty: Ty,
    pub mutable: bool,
    pub parameter: bool,
    pub synthetic: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirBasicBlock {
    pub id: FirBlockId,
    pub instructions: Vec<FirInstruction>,
    pub terminator: Option<FirTerminator>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirInstruction {
    pub span: Span,
    pub result: Option<FirValueId>,
    pub kind: FirInstructionKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FirInstructionKind {
    Const { value: FirConst },
    FunctionRef { target: DefId },
    LoadGlobal { global: DefId },
    Load { place: FirPlace },
    Store { place: FirPlace, value: FirValueId },
    Unary { op: FirUnaryOp, value: FirValueId },
    Binary {
        op: BinaryOp,
        overflow: Option<OverflowMode>,
        left: FirValueId,
        right: FirValueId,
    },
    Convert { value: FirValueId, target: Ty },
    MakeArray { items: Vec<FirValueId> },
    MakeAggregate {
        ty: Ty,
        variant: Option<String>,
        fields: Vec<(String, FirValueId)>,
    },
    MakeNone,
    MakeSome { value: FirValueId },
    Variant { ty: Ty, name: String },
    VariantIs { value: FirValueId, name: String },
    ExtractField { base: FirValueId, field: String },
    Len { value: FirValueId },
    BoundsCheck { index: FirValueId, len: FirValueId },
    IndexUnchecked { base: FirValueId, index: FirValueId },
    AddressOf { place: FirPlace, mutable: bool },
    Call {
        target: DefId,
        args: Vec<FirValueId>,
        tail: bool,
    },
    CallIndirect {
        callee: FirValueId,
        args: Vec<FirValueId>,
        tail: bool,
    },
    ResultIsOk { value: FirValueId },
    ResultUnwrapOk { value: FirValueId },
    ResultUnwrapErr { value: FirValueId },
    MakeResultErr { error: FirValueId },
    OptionIsSome { value: FirValueId },
    OptionUnwrap { value: FirValueId },
    Poison,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum FirPlace {
    Local { local: FirLocalId },
    Field { base: Box<FirPlace>, field: String },
    Index { base: Box<FirPlace>, index: FirValueId },
    Deref { address: FirValueId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FirUnaryOp {
    Neg,
    Not,
    BitNot,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "const", rename_all = "snake_case")]
pub enum FirConst {
    Integer { text: String },
    Float { text: String },
    Bool { value: bool },
    Char { value: char },
    String { value: String },
    CString { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "term", rename_all = "snake_case")]
pub enum FirTerminator {
    Goto { target: FirBlockId },
    Branch {
        condition: FirValueId,
        then_block: FirBlockId,
        else_block: FirBlockId,
    },
    Return { value: Option<FirValueId> },
    Unreachable,
}

pub fn lower_fir(bodies: &BodyHirOutput, typed: &TypeCheckOutput) -> FirOutput {
    let mut output = FirOutput::default();

    for (owner, ty) in &typed.global_types {
        output.module.globals.insert(
            *owner,
            FirGlobal {
                owner: *owner,
                ty: ty.clone(),
                constant: typed.constants.get(owner).cloned(),
            },
        );
    }

    for (owner, body) in &bodies.functions {
        let Some(typed_body) = typed.functions.get(owner) else {
            output.diagnostics.push(FirDiagnostic {
                span: body.block.span,
                code: "fir/missing-typed-body".into(),
                message: format!("missing typed HIR for function {owner:?}"),
            });
            continue;
        };
        let (function, mut diagnostics) = FunctionLowerer::new(
            body,
            typed_body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        diagnostics.extend(verify_fir_function(&function));
        output.diagnostics.extend(diagnostics);
        output.module.functions.insert(*owner, function);
    }

    output
}

fn function_overflow_mode(typed: &TypeCheckOutput, owner: DefId) -> OverflowMode {
    let target = MetadataTarget::Item { owner };
    for metadata in typed.metadata.named(&target, "overflow") {
        if let Some(MetadataArg::Value(value)) = metadata.arguments.first() {
            match value {
                FdnValue::Symbol { name } | FdnValue::Keyword { name } if name == "wrap" => {
                    return OverflowMode::Wrapping;
                }
                FdnValue::Symbol { name } | FdnValue::Keyword { name } if name == "checked" => {
                    return OverflowMode::Checked;
                }
                _ => {}
            }
        }
    }
    OverflowMode::Checked
}

#[derive(Debug, Clone)]
enum Deferred {
    Expr(HirExpr),
    Block(HirBlock),
}

#[derive(Debug, Clone, Copy)]
struct LoopTargets {
    break_target: FirBlockId,
    continue_target: FirBlockId,
    cleanup_depth: usize,
}

struct FunctionLowerer<'a> {
    body: &'a crate::body_hir::HirBody,
    typed: &'a TypedBody,
    bodies: &'a BodyHirOutput,
    all_typed: &'a TypeCheckOutput,
    exprs: BTreeMap<ExprId, &'a TypedExpr>,
    local_map: BTreeMap<LocalId, FirLocalId>,
    overflow: OverflowMode,
    function: FirFunction,
    current: FirBlockId,
    next_value: u32,
    next_local: u32,
    cleanup_scopes: Vec<Vec<Deferred>>,
    loops: Vec<LoopTargets>,
    diagnostics: Vec<FirDiagnostic>,
    in_cleanup: bool,
}

impl<'a> FunctionLowerer<'a> {
    fn new(
        body: &'a crate::body_hir::HirBody,
        typed: &'a TypedBody,
        bodies: &'a BodyHirOutput,
        all_typed: &'a TypeCheckOutput,
        overflow: OverflowMode,
    ) -> Self {
        let mut exprs = BTreeMap::new();
        for expr in &typed.expressions {
            exprs.insert(expr.id, expr);
        }
        let entry = FirBlockId(0);
        let mut function = FirFunction {
            owner: body.owner,
            params: Vec::new(),
            return_type: typed.return_type.clone(),
            locals: BTreeMap::new(),
            entry,
            blocks: vec![FirBasicBlock {
                id: entry,
                instructions: Vec::new(),
                terminator: None,
            }],
            value_types: BTreeMap::new(),
        };
        let mut local_map = BTreeMap::new();
        let param_set = typed.params.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
        let mut next_local = 0;
        for local in &body.locals {
            let id = FirLocalId(next_local);
            next_local += 1;
            let ty = typed
                .local_types
                .get(&local.id)
                .cloned()
                .unwrap_or(Ty::Unknown);
            function.locals.insert(
                id,
                FirLocal {
                    id,
                    source: Some(local.id),
                    ty,
                    mutable: local.mutable,
                    parameter: param_set.contains(&local.id),
                    synthetic: false,
                },
            );
            local_map.insert(local.id, id);
        }
        for (source, _) in &typed.params {
            if let Some(local) = local_map.get(source).copied() {
                function.params.push(local);
            }
        }
        Self {
            body,
            typed,
            bodies,
            all_typed,
            exprs,
            local_map,
            overflow,
            function,
            current: entry,
            next_value: 0,
            next_local,
            cleanup_scopes: Vec::new(),
            loops: Vec::new(),
            diagnostics: Vec::new(),
            in_cleanup: false,
        }
    }

    fn lower(mut self) -> (FirFunction, Vec<FirDiagnostic>) {
        self.lower_block(&self.body.block);
        if !self.terminated() {
            if self.function.return_type == Ty::Void {
                self.terminate(FirTerminator::Return { value: None });
            } else {
                self.diagnostic(
                    self.body.block.span,
                    "fir/missing-return",
                    "control reaches the end of a non-void function",
                );
                self.terminate(FirTerminator::Unreachable);
            }
        }
        (self.function, self.diagnostics)
    }

    fn diagnostic(&mut self, span: Span, code: &str, message: impl Into<String>) {
        self.diagnostics.push(FirDiagnostic {
            span,
            code: code.into(),
            message: message.into(),
        });
    }

    fn block_mut(&mut self) -> &mut FirBasicBlock {
        &mut self.function.blocks[self.current.0 as usize]
    }

    fn terminated(&self) -> bool {
        self.function.blocks[self.current.0 as usize]
            .terminator
            .is_some()
    }

    fn new_block(&mut self) -> FirBlockId {
        let id = FirBlockId(self.function.blocks.len() as u32);
        self.function.blocks.push(FirBasicBlock {
            id,
            instructions: Vec::new(),
            terminator: None,
        });
        id
    }

    fn switch_to(&mut self, block: FirBlockId) {
        self.current = block;
    }

    fn terminate(&mut self, term: FirTerminator) {
        if self.terminated() {
            return;
        }
        self.block_mut().terminator = Some(term);
    }

    fn emit_value(&mut self, span: Span, ty: Ty, kind: FirInstructionKind) -> FirValueId {
        if !fir_type_is_concrete(&ty) {
            self.diagnostic(
                span,
                "fir/non-concrete-type",
                format!("FIR received non-concrete semantic type {ty:?}"),
            );
        }
        let value = FirValueId(self.next_value);
        self.next_value += 1;
        self.function.value_types.insert(value, ty);
        self.block_mut().instructions.push(FirInstruction {
            span,
            result: Some(value),
            kind,
        });
        value
    }

    fn emit_void(&mut self, span: Span, kind: FirInstructionKind) {
        self.block_mut().instructions.push(FirInstruction {
            span,
            result: None,
            kind,
        });
    }

    fn synthetic_local(&mut self, ty: Ty) -> FirLocalId {
        let id = FirLocalId(self.next_local);
        self.next_local += 1;
        self.function.locals.insert(
            id,
            FirLocal {
                id,
                source: None,
                ty,
                mutable: true,
                parameter: false,
                synthetic: true,
            },
        );
        id
    }

    fn typed_expr(&mut self, expr: &HirExpr) -> Option<&'a TypedExpr> {
        if let Some(typed) = self.exprs.get(&expr.id).copied() {
            Some(typed)
        } else {
            self.diagnostic(
                expr.span,
                "fir/missing-expression-type",
                format!("typed HIR has no entry for expression {:?}", expr.id),
            );
            None
        }
    }

    fn expr_ty(&mut self, expr: &HirExpr) -> Ty {
        self.typed_expr(expr)
            .map(|typed| typed.ty.clone())
            .unwrap_or(Ty::Error)
    }

    fn lower_block(&mut self, block: &HirBlock) {
        self.cleanup_scopes.push(Vec::new());
        let scope = self.cleanup_scopes.len() - 1;
        for stmt in &block.statements {
            if self.terminated() {
                break;
            }
            self.lower_stmt(stmt);
        }
        if !self.terminated() {
            self.emit_scope_cleanups(scope);
        }
        self.cleanup_scopes.pop();
    }

    fn lower_stmt(&mut self, stmt: &HirStmt) {
        match &stmt.kind {
            HirStmtKind::Value {
                constant,
                pattern,
                value,
                ..
            } => {
                if *constant {
                    return;
                }
                let ty = self.expr_ty(value);
                let value_id = self.lower_expr(value);
                self.bind_irrefutable_pattern(pattern, value_id, &ty);
            }
            HirStmtKind::Assignment { target, value } => {
                let value = self.lower_expr(value);
                if let Some(place) = self.lower_place(target) {
                    self.emit_void(stmt.span, FirInstructionKind::Store { place, value });
                }
            }
            HirStmtKind::Expr { expr } => {
                self.lower_expr(expr);
            }
            HirStmtKind::Return { tail, value } => {
                if self.in_cleanup {
                    self.diagnostic(
                        stmt.span,
                        "fir/control-in-cleanup",
                        "return from a defer cleanup is not lowered in FIR v1",
                    );
                    self.terminate(FirTerminator::Unreachable);
                    return;
                }
                let result = value.as_ref().map(|expr| {
                    if *tail && matches!(expr.kind, HirExprKind::Call { .. }) {
                        self.lower_call_expression(expr, true)
                    } else {
                        self.lower_expr(expr)
                    }
                });
                self.emit_cleanups_from(0);
                if !self.terminated() {
                    self.terminate(FirTerminator::Return { value: result });
                }
            }
            HirStmtKind::If {
                condition,
                then_block,
                else_branch,
            } => self.lower_if(condition, then_block, else_branch.as_deref()),
            HirStmtKind::While { condition, body } => self.lower_while(condition, body),
            HirStmtKind::ForC {
                init,
                condition,
                step,
                body,
            } => self.lower_for_c(init.as_deref(), condition.as_ref(), step.as_deref(), body),
            HirStmtKind::ForEach {
                pattern,
                iterable,
                body,
                ..
            } => self.lower_for_each(pattern, iterable, body),
            HirStmtKind::Break => self.lower_loop_exit(stmt.span, true),
            HirStmtKind::Continue => self.lower_loop_exit(stmt.span, false),
            HirStmtKind::DeferExpr { expr } => {
                if let Some(scope) = self.cleanup_scopes.last_mut() {
                    scope.push(Deferred::Expr(expr.clone()));
                }
            }
            HirStmtKind::DeferBlock { block } => {
                if let Some(scope) = self.cleanup_scopes.last_mut() {
                    scope.push(Deferred::Block(block.clone()));
                }
            }
            HirStmtKind::Unsafe { block } | HirStmtKind::Block { block } => self.lower_block(block),
            HirStmtKind::WithContext { .. } => self.diagnostic(
                stmt.span,
                "fir/context-not-resolved",
                "with-context semantics are not resolved in typed HIR yet",
            ),
            HirStmtKind::Select { .. } => self.diagnostic(
                stmt.span,
                "fir/select-not-resolved",
                "select/channel semantics are not typed strongly enough for FIR lowering yet",
            ),
        }
    }

    fn lower_if(&mut self, condition: &HirExpr, then_block: &HirBlock, else_stmt: Option<&HirStmt>) {
        let condition = self.lower_expr(condition);
        let then_id = self.new_block();
        let else_id = self.new_block();
        let join_id = self.new_block();
        self.terminate(FirTerminator::Branch {
            condition,
            then_block: then_id,
            else_block: else_id,
        });

        self.switch_to(then_id);
        self.lower_block(then_block);
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: join_id });
        }

        self.switch_to(else_id);
        if let Some(stmt) = else_stmt {
            self.lower_stmt(stmt);
        }
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: join_id });
        }

        self.switch_to(join_id);
    }

    fn lower_while(&mut self, condition: &HirExpr, body: &HirBlock) {
        let cond_id = self.new_block();
        let body_id = self.new_block();
        let exit_id = self.new_block();
        self.terminate(FirTerminator::Goto { target: cond_id });
        self.switch_to(cond_id);
        let condition = self.lower_expr(condition);
        self.terminate(FirTerminator::Branch {
            condition,
            then_block: body_id,
            else_block: exit_id,
        });
        self.loops.push(LoopTargets {
            break_target: exit_id,
            continue_target: cond_id,
            cleanup_depth: self.cleanup_scopes.len(),
        });
        self.switch_to(body_id);
        self.lower_block(body);
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: cond_id });
        }
        self.loops.pop();
        self.switch_to(exit_id);
    }

    fn lower_for_c(
        &mut self,
        init: Option<&HirStmt>,
        condition: Option<&HirExpr>,
        step: Option<&HirStmt>,
        body: &HirBlock,
    ) {
        if let Some(init) = init {
            self.lower_stmt(init);
        }
        if self.terminated() {
            return;
        }
        let cond_id = self.new_block();
        let body_id = self.new_block();
        let step_id = self.new_block();
        let exit_id = self.new_block();
        self.terminate(FirTerminator::Goto { target: cond_id });
        self.switch_to(cond_id);
        if let Some(condition) = condition {
            let condition = self.lower_expr(condition);
            self.terminate(FirTerminator::Branch {
                condition,
                then_block: body_id,
                else_block: exit_id,
            });
        } else {
            self.terminate(FirTerminator::Goto { target: body_id });
        }
        self.loops.push(LoopTargets {
            break_target: exit_id,
            continue_target: step_id,
            cleanup_depth: self.cleanup_scopes.len(),
        });
        self.switch_to(body_id);
        self.lower_block(body);
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: step_id });
        }
        self.switch_to(step_id);
        if let Some(step) = step {
            self.lower_stmt(step);
        }
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: cond_id });
        }
        self.loops.pop();
        self.switch_to(exit_id);
    }

    fn lower_for_each(&mut self, pattern: &HirPattern, iterable: &HirExpr, body: &HirBlock) {
        let iterable_ty = self.expr_ty(iterable);
        let iterable_value = self.lower_expr(iterable);
        let iterable_local = self.synthetic_local(iterable_ty.clone());
        self.emit_void(
            iterable.span,
            FirInstructionKind::Store {
                place: FirPlace::Local {
                    local: iterable_local,
                },
                value: iterable_value,
            },
        );
        let usize_ty = Ty::Int {
            signed: false,
            width: IntWidth::Pointer,
        };
        let index_local = self.synthetic_local(usize_ty.clone());
        let zero = self.emit_value(
            iterable.span,
            usize_ty.clone(),
            FirInstructionKind::Const {
                value: FirConst::Integer { text: "0".into() },
            },
        );
        self.emit_void(
            iterable.span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local: index_local },
                value: zero,
            },
        );

        let cond_id = self.new_block();
        let body_id = self.new_block();
        let step_id = self.new_block();
        let exit_id = self.new_block();
        self.terminate(FirTerminator::Goto { target: cond_id });
        self.switch_to(cond_id);
        let index = self.emit_value(
            iterable.span,
            usize_ty.clone(),
            FirInstructionKind::Load {
                place: FirPlace::Local { local: index_local },
            },
        );
        let container = self.emit_value(
            iterable.span,
            iterable_ty.clone(),
            FirInstructionKind::Load {
                place: FirPlace::Local {
                    local: iterable_local,
                },
            },
        );
        let len = self.emit_len(iterable.span, container, &iterable_ty);
        let condition = self.emit_value(
            iterable.span,
            Ty::Bool,
            FirInstructionKind::Binary {
                op: BinaryOp::Less,
                overflow: None,
                left: index,
                right: len,
            },
        );
        self.terminate(FirTerminator::Branch {
            condition,
            then_block: body_id,
            else_block: exit_id,
        });

        self.loops.push(LoopTargets {
            break_target: exit_id,
            continue_target: step_id,
            cleanup_depth: self.cleanup_scopes.len(),
        });
        self.switch_to(body_id);
        let element_ty = match &iterable_ty {
            Ty::Array { element, .. } | Ty::Slice { element, .. } => element.as_ref().clone(),
            _ => Ty::Error,
        };
        let element = self.emit_value(
            iterable.span,
            element_ty.clone(),
            FirInstructionKind::IndexUnchecked {
                base: container,
                index,
            },
        );
        self.bind_irrefutable_pattern(pattern, element, &element_ty);
        self.lower_block(body);
        if !self.terminated() {
            self.terminate(FirTerminator::Goto { target: step_id });
        }

        self.switch_to(step_id);
        let old = self.emit_value(
            iterable.span,
            usize_ty.clone(),
            FirInstructionKind::Load {
                place: FirPlace::Local { local: index_local },
            },
        );
        let one = self.emit_value(
            iterable.span,
            usize_ty.clone(),
            FirInstructionKind::Const {
                value: FirConst::Integer { text: "1".into() },
            },
        );
        let next = self.emit_value(
            iterable.span,
            usize_ty,
            FirInstructionKind::Binary {
                op: BinaryOp::Add,
                overflow: Some(OverflowMode::Checked),
                left: old,
                right: one,
            },
        );
        self.emit_void(
            iterable.span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local: index_local },
                value: next,
            },
        );
        self.terminate(FirTerminator::Goto { target: cond_id });
        self.loops.pop();
        self.switch_to(exit_id);
    }

    fn lower_loop_exit(&mut self, span: Span, is_break: bool) {
        if self.in_cleanup {
            self.diagnostic(
                span,
                "fir/control-in-cleanup",
                "break/continue from a defer cleanup is not lowered in FIR v1",
            );
            self.terminate(FirTerminator::Unreachable);
            return;
        }
        let Some(targets) = self.loops.last().copied() else {
            self.diagnostic(span, "fir/loop-control", "break/continue outside a loop");
            self.terminate(FirTerminator::Unreachable);
            return;
        };
        self.emit_cleanups_from(targets.cleanup_depth);
        if !self.terminated() {
            self.terminate(FirTerminator::Goto {
                target: if is_break {
                    targets.break_target
                } else {
                    targets.continue_target
                },
            });
        }
    }

    fn emit_scope_cleanups(&mut self, scope: usize) {
        if let Some(values) = self.cleanup_scopes.get(scope).cloned() {
            self.emit_deferred(values);
        }
    }

    fn emit_cleanups_from(&mut self, depth: usize) {
        let scopes = self.cleanup_scopes[depth..].to_vec();
        for values in scopes.into_iter().rev() {
            self.emit_deferred(values);
            if self.terminated() {
                break;
            }
        }
    }

    fn emit_deferred(&mut self, values: Vec<Deferred>) {
        for deferred in values.into_iter().rev() {
            if self.terminated() {
                break;
            }
            let previous = self.in_cleanup;
            self.in_cleanup = true;
            match deferred {
                Deferred::Expr(expr) => {
                    self.lower_expr(&expr);
                }
                Deferred::Block(block) => self.lower_block(&block),
            }
            self.in_cleanup = previous;
        }
    }

    fn lower_expr(&mut self, expr: &HirExpr) -> FirValueId {
        let Some(typed) = self.typed_expr(expr).cloned() else {
            return self.poison(expr.span, Ty::Error);
        };
        self.lower_expr_kind(expr, &typed.kind, typed.ty.clone())
    }

    fn lower_expr_kind(
        &mut self,
        expr: &HirExpr,
        kind: &TypedExprKind,
        result_ty: Ty,
    ) -> FirValueId {
        match kind {
            TypedExprKind::OptionalPromote {
                source_type, inner, ..
            } => {
                let inner = self.lower_expr_kind(expr, inner, source_type.clone());
                self.emit_value(
                    expr.span,
                    result_ty,
                    FirInstructionKind::MakeSome { value: inner },
                )
            }
            TypedExprKind::ResolvedCall {
                target,
                method,
                receiver,
                argument_parameters,
                ..
            } => self.lower_resolved_call(
                expr,
                *target,
                *method,
                *receiver,
                argument_parameters,
                result_ty,
                false,
            ),
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                ..
            } => self.lower_try(expr, source_error, target_error, result_ty),
            TypedExprKind::Source { .. } => self.lower_source_expr(expr, result_ty),
        }
    }

    fn lower_source_expr(&mut self, expr: &HirExpr, ty: Ty) -> FirValueId {
        match &expr.kind {
            HirExprKind::Integer { text } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::Integer { text: text.clone() },
                },
            ),
            HirExprKind::Float { text } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::Float { text: text.clone() },
                },
            ),
            HirExprKind::Character { value } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::Char { value: *value },
                },
            ),
            HirExprKind::String { value } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::String {
                        value: value.clone(),
                    },
                },
            ),
            HirExprKind::CString { value } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::CString {
                        value: value.clone(),
                    },
                },
            ),
            HirExprKind::Bool { value } => self.emit_value(
                expr.span,
                ty,
                FirInstructionKind::Const {
                    value: FirConst::Bool { value: *value },
                },
            ),
            HirExprKind::None => self.emit_value(expr.span, ty, FirInstructionKind::MakeNone),
            HirExprKind::Name { reference } => self.lower_name(expr.span, reference.root, ty),
            HirExprKind::Qualified { name, .. } => self.emit_value(
                expr.span,
                ty.clone(),
                FirInstructionKind::Variant {
                    ty,
                    name: name.clone(),
                },
            ),
            HirExprKind::Array { items } => {
                let items = items.iter().map(|item| self.lower_expr(item)).collect();
                self.emit_value(expr.span, ty, FirInstructionKind::MakeArray { items })
            }
            HirExprKind::StructInit {
                variant, fields, ..
            } => {
                let fields = fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.lower_expr(value)))
                    .collect();
                self.emit_value(
                    expr.span,
                    ty.clone(),
                    FirInstructionKind::MakeAggregate {
                        ty,
                        variant: variant.clone(),
                        fields,
                    },
                )
            }
            HirExprKind::Unary { op, value } => self.lower_unary(expr.span, *op, value, ty),
            HirExprKind::Binary { op, left, right } => {
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    self.lower_short_circuit(expr.span, *op, left, right)
                } else {
                    let left = self.lower_expr(left);
                    let right = self.lower_expr(right);
                    let overflow = binary_overflow(*op, self.overflow);
                    self.emit_value(
                        expr.span,
                        ty,
                        FirInstructionKind::Binary {
                            op: *op,
                            overflow,
                            left,
                            right,
                        },
                    )
                }
            }
            HirExprKind::Call { .. } => self.lower_call_expression(expr, false),
            HirExprKind::TypeCall { args, .. } => {
                let Some(value) = first_positional(args) else {
                    self.diagnostic(
                        expr.span,
                        "fir/conversion-arity",
                        "type conversion requires one positional operand",
                    );
                    return self.poison(expr.span, ty);
                };
                let value = self.lower_expr(value);
                self.emit_value(
                    expr.span,
                    ty.clone(),
                    FirInstructionKind::Convert { value, target: ty },
                )
            }
            HirExprKind::Index { base, index } => {
                let base_ty = self.expr_ty(base);
                let base_value = self.lower_expr(base);
                let index_value = self.lower_expr(index);
                let len = self.emit_len(expr.span, base_value, &base_ty);
                self.emit_void(
                    expr.span,
                    FirInstructionKind::BoundsCheck {
                        index: index_value,
                        len,
                    },
                );
                self.emit_value(
                    expr.span,
                    ty,
                    FirInstructionKind::IndexUnchecked {
                        base: base_value,
                        index: index_value,
                    },
                )
            }
            HirExprKind::Member { base, name } => {
                if let Some(place) = self.try_place(base) {
                    let place = FirPlace::Field {
                        base: Box::new(place),
                        field: name.clone(),
                    };
                    self.emit_value(expr.span, ty, FirInstructionKind::Load { place })
                } else {
                    let base = self.lower_expr(base);
                    self.emit_value(
                        expr.span,
                        ty,
                        FirInstructionKind::ExtractField {
                            base,
                            field: name.clone(),
                        },
                    )
                }
            }
            HirExprKind::Try { .. } => {
                self.diagnostic(
                    expr.span,
                    "fir/unresolved-try",
                    "typed HIR did not retain resolved `?` semantics",
                );
                self.poison(expr.span, ty)
            }
            HirExprKind::Match { .. } => {
                self.diagnostic(
                    expr.span,
                    "fir/pattern-decision-tree-missing",
                    "match reached FIR without a lowered pattern decision tree",
                );
                self.poison(expr.span, ty)
            }
            HirExprKind::Closure { .. } => {
                self.diagnostic(
                    expr.span,
                    "fir/closure-environment-missing",
                    "closure capture types/environment layout are not explicit enough in typed HIR yet",
                );
                self.poison(expr.span, ty)
            }
            HirExprKind::Keyword { .. } | HirExprKind::ReaderForm { .. } | HirExprKind::Error => {
                self.diagnostic(
                    expr.span,
                    "fir/unresolved-expression",
                    "source-only or unresolved expression reached FIR",
                );
                self.poison(expr.span, ty)
            }
        }
    }

    fn lower_call_expression(&mut self, expr: &HirExpr, tail: bool) -> FirValueId {
        let Some(typed) = self.typed_expr(expr).cloned() else {
            return self.poison(expr.span, Ty::Error);
        };
        match typed.kind {
            TypedExprKind::ResolvedCall {
                target,
                method,
                receiver,
                argument_parameters,
                ..
            } => self.lower_resolved_call(
                expr,
                target,
                method,
                receiver,
                &argument_parameters,
                typed.ty,
                tail,
            ),
            TypedExprKind::OptionalPromote { inner, source_type, .. } => {
                let value = self.lower_expr_kind(expr, &inner, source_type);
                self.emit_value(
                    expr.span,
                    typed.ty,
                    FirInstructionKind::MakeSome { value },
                )
            }
            _ => {
                let HirExprKind::Call { callee, args } = &expr.kind else {
                    return self.lower_expr(expr);
                };
                if args.iter().any(|arg| matches!(arg, HirCallArg::Named { .. })) {
                    self.diagnostic(
                        expr.span,
                        "fir/indirect-named-call",
                        "named arguments on indirect calls need semantic parameter mapping",
                    );
                }
                let callee = self.lower_expr(callee);
                let args = args.iter().map(arg_value).map(|arg| self.lower_expr(arg)).collect();
                self.emit_value(
                    expr.span,
                    typed.ty,
                    FirInstructionKind::CallIndirect { callee, args, tail },
                )
            }
        }
    }

    fn lower_resolved_call(
        &mut self,
        expr: &HirExpr,
        target: DefId,
        method: bool,
        receiver: Option<ResolvedReceiver>,
        argument_parameters: &[usize],
        result_ty: Ty,
        tail: bool,
    ) -> FirValueId {
        let HirExprKind::Call { callee, args } = &expr.kind else {
            self.diagnostic(expr.span, "fir/call-shape", "resolved call is not a call HIR node");
            return self.poison(expr.span, result_ty);
        };
        let Some(target_body) = self.all_typed.functions.get(&target) else {
            self.diagnostic(
                expr.span,
                "fir/call-target-signature",
                format!("missing typed signature for call target {target:?}"),
            );
            return self.poison(expr.span, result_ty);
        };
        let mut placed: Vec<Option<FirValueId>> = vec![None; target_body.params.len()];
        let offset = if method { 1 } else { 0 };

        if method {
            let HirExprKind::Member { base, .. } = &callee.kind else {
                self.diagnostic(expr.span, "fir/method-shape", "resolved method call has no member receiver");
                return self.poison(expr.span, result_ty);
            };
            let receiver_value = match receiver.unwrap_or(ResolvedReceiver::Value) {
                ResolvedReceiver::Value => self.lower_expr(base),
                ResolvedReceiver::SharedReference | ResolvedReceiver::MutableReference => {
                    let expected_ref = target_body
                        .params
                        .first()
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(Ty::Error);
                    let base_ty = self.expr_ty(base);
                    if matches!(base_ty, Ty::Reference { .. }) {
                        self.lower_expr(base)
                    } else if let Some(place) = self.lower_place(base) {
                        self.emit_value(
                            base.span,
                            expected_ref,
                            FirInstructionKind::AddressOf {
                                place,
                                mutable: matches!(receiver, Some(ResolvedReceiver::MutableReference)),
                            },
                        )
                    } else {
                        self.diagnostic(
                            base.span,
                            "fir/method-receiver-place",
                            "reference receiver was resolved but the receiver is not a lowerable place",
                        );
                        self.poison(base.span, expected_ref)
                    }
                }
            };
            if !placed.is_empty() {
                placed[0] = Some(receiver_value);
            }
        }

        for (written, arg) in args.iter().enumerate() {
            let Some(parameter) = argument_parameters.get(written).copied() else {
                continue;
            };
            let parameter = parameter + offset;
            if parameter < placed.len() {
                placed[parameter] = Some(self.lower_expr(arg_value(arg)));
            }
        }

        if placed.iter().any(Option::is_none) {
            self.diagnostic(
                expr.span,
                "fir/default-argument-not-materialized",
                "call uses omitted default arguments; typed HIR validates defaults but does not yet materialize them at the call site",
            );
            for (index, slot) in placed.iter_mut().enumerate() {
                if slot.is_none() {
                    let ty = target_body
                        .params
                        .get(index)
                        .map(|(_, ty)| ty.clone())
                        .unwrap_or(Ty::Error);
                    *slot = Some(self.poison(expr.span, ty));
                }
            }
        }
        let args = placed.into_iter().flatten().collect();
        self.emit_value(
            expr.span,
            result_ty,
            FirInstructionKind::Call {
                target,
                args,
                tail,
            },
        )
    }

    fn lower_try(
        &mut self,
        expr: &HirExpr,
        _source_error: &Ty,
        _target_error: &Ty,
        result_ty: Ty,
    ) -> FirValueId {
        let HirExprKind::Try { value } = &expr.kind else {
            self.diagnostic(expr.span, "fir/try-shape", "resolved try is not a try HIR node");
            return self.poison(expr.span, result_ty);
        };
        let source = self.lower_expr(value);
        let is_ok = self.emit_value(
            expr.span,
            Ty::Bool,
            FirInstructionKind::ResultIsOk { value: source },
        );
        let ok_block = self.new_block();
        let err_block = self.new_block();
        let join_block = self.new_block();
        self.terminate(FirTerminator::Branch {
            condition: is_ok,
            then_block: ok_block,
            else_block: err_block,
        });

        self.switch_to(err_block);
        let error_ty = match self.expr_ty(value) {
            Ty::Result { error, .. } => *error,
            _ => Ty::Error,
        };
        let error = self.emit_value(
            expr.span,
            error_ty,
            FirInstructionKind::ResultUnwrapErr { value: source },
        );
        let propagated = self.emit_value(
            expr.span,
            self.function.return_type.clone(),
            FirInstructionKind::MakeResultErr { error },
        );
        self.emit_cleanups_from(0);
        if !self.terminated() {
            self.terminate(FirTerminator::Return {
                value: Some(propagated),
            });
        }

        self.switch_to(ok_block);
        let ok = self.emit_value(
            expr.span,
            result_ty,
            FirInstructionKind::ResultUnwrapOk { value: source },
        );
        self.terminate(FirTerminator::Goto { target: join_block });
        self.switch_to(join_block);
        ok
    }

    fn lower_name(&mut self, span: Span, name: ResolvedName, ty: Ty) -> FirValueId {
        match name {
            ResolvedName::Local(local) => {
                if let Some(value) = self.typed.local_constants.get(&local).cloned() {
                    return self.emit_const_value(span, ty, value);
                }
                let Some(local) = self.local_map.get(&local).copied() else {
                    self.diagnostic(span, "fir/local", "unknown local in FIR lowering");
                    return self.poison(span, ty);
                };
                self.emit_value(
                    span,
                    ty,
                    FirInstructionKind::Load {
                        place: FirPlace::Local { local },
                    },
                )
            }
            ResolvedName::Def(def) => {
                if let Some(value) = self.all_typed.constants.get(&def).cloned() {
                    self.emit_const_value(span, ty, value)
                } else if self.all_typed.global_types.contains_key(&def) {
                    self.emit_value(span, ty, FirInstructionKind::LoadGlobal { global: def })
                } else if self.all_typed.functions.contains_key(&def) {
                    self.emit_value(span, ty, FirInstructionKind::FunctionRef { target: def })
                } else {
                    self.diagnostic(
                        span,
                        "fir/unresolved-def",
                        format!("definition {def:?} has no FIR value category"),
                    );
                    self.poison(span, ty)
                }
            }
            _ => {
                self.diagnostic(span, "fir/unresolved-name", "unresolved name reached FIR");
                self.poison(span, ty)
            }
        }
    }

    fn emit_const_value(&mut self, span: Span, ty: Ty, value: ConstValue) -> FirValueId {
        let value = match value {
            ConstValue::Integer { value } => FirConst::Integer {
                text: value.to_string(),
            },
            ConstValue::Bool { value } => FirConst::Bool { value },
            ConstValue::Char { value } => FirConst::Char { value },
        };
        self.emit_value(span, ty, FirInstructionKind::Const { value })
    }

    fn lower_unary(&mut self, span: Span, op: UnaryOp, value: &HirExpr, ty: Ty) -> FirValueId {
        match op {
            UnaryOp::AddressOf | UnaryOp::AddressOfMut => {
                let Some(place) = self.lower_place(value) else {
                    return self.poison(span, ty);
                };
                self.emit_value(
                    span,
                    ty,
                    FirInstructionKind::AddressOf {
                        place,
                        mutable: matches!(op, UnaryOp::AddressOfMut),
                    },
                )
            }
            UnaryOp::Deref => {
                let address = self.lower_expr(value);
                self.emit_value(
                    span,
                    ty,
                    FirInstructionKind::Load {
                        place: FirPlace::Deref { address },
                    },
                )
            }
            UnaryOp::Neg | UnaryOp::Not | UnaryOp::BitNot => {
                let value = self.lower_expr(value);
                let op = match op {
                    UnaryOp::Neg => FirUnaryOp::Neg,
                    UnaryOp::Not => FirUnaryOp::Not,
                    UnaryOp::BitNot => FirUnaryOp::BitNot,
                    _ => unreachable!(),
                };
                self.emit_value(span, ty, FirInstructionKind::Unary { op, value })
            }
        }
    }

    fn lower_short_circuit(
        &mut self,
        span: Span,
        op: BinaryOp,
        left: &HirExpr,
        right: &HirExpr,
    ) -> FirValueId {
        let temp = self.synthetic_local(Ty::Bool);
        let left = self.lower_expr(left);
        let rhs = self.new_block();
        let short = self.new_block();
        let join = self.new_block();
        let (then_block, else_block, short_value) = if op == BinaryOp::LogicalAnd {
            (rhs, short, false)
        } else {
            (short, rhs, true)
        };
        self.terminate(FirTerminator::Branch {
            condition: left,
            then_block,
            else_block,
        });
        self.switch_to(short);
        let value = self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Const {
                value: FirConst::Bool { value: short_value },
            },
        );
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local: temp },
                value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });
        self.switch_to(rhs);
        let value = self.lower_expr(right);
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local: temp },
                value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });
        self.switch_to(join);
        self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Load {
                place: FirPlace::Local { local: temp },
            },
        )
    }

    fn lower_place(&mut self, expr: &HirExpr) -> Option<FirPlace> {
        let place = self.try_place(expr);
        if place.is_none() {
            self.diagnostic(expr.span, "fir/place", "expression is not a lowerable place");
        }
        place
    }

    fn try_place(&mut self, expr: &HirExpr) -> Option<FirPlace> {
        match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Local(local) => self
                    .local_map
                    .get(&local)
                    .copied()
                    .map(|local| FirPlace::Local { local }),
                _ => None,
            },
            HirExprKind::Member { base, name } => self.try_place(base).map(|base| FirPlace::Field {
                base: Box::new(base),
                field: name.clone(),
            }),
            HirExprKind::Index { base, index } => {
                let base_place = self.try_place(base)?;
                let base_ty = self.expr_ty(base);
                let base_value = self.emit_value(
                    base.span,
                    base_ty.clone(),
                    FirInstructionKind::Load {
                        place: base_place.clone(),
                    },
                );
                let index_value = self.lower_expr(index);
                let len = self.emit_len(expr.span, base_value, &base_ty);
                self.emit_void(
                    expr.span,
                    FirInstructionKind::BoundsCheck {
                        index: index_value,
                        len,
                    },
                );
                Some(FirPlace::Index {
                    base: Box::new(base_place),
                    index: index_value,
                })
            }
            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => Some(FirPlace::Deref {
                address: self.lower_expr(value),
            }),
            _ => None,
        }
    }

    fn emit_len(&mut self, span: Span, value: FirValueId, ty: &Ty) -> FirValueId {
        let usize_ty = Ty::Int {
            signed: false,
            width: IntWidth::Pointer,
        };
        match ty {
            Ty::Array {
                length: Some(length),
                ..
            } => self.emit_value(
                span,
                usize_ty,
                FirInstructionKind::Const {
                    value: FirConst::Integer {
                        text: length.to_string(),
                    },
                },
            ),
            Ty::Slice { .. } | Ty::Str => {
                self.emit_value(span, usize_ty, FirInstructionKind::Len { value })
            }
            _ => {
                self.diagnostic(span, "fir/len", format!("cannot obtain length of {ty:?}"));
                self.poison(span, usize_ty)
            }
        }
    }

    fn bind_irrefutable_pattern(&mut self, pattern: &HirPattern, value: FirValueId, ty: &Ty) {
        match &pattern.kind {
            HirPatternKind::Wildcard => {}
            HirPatternKind::Binding { local, .. } => self.store_local(pattern.span, *local, value),
            HirPatternKind::As { local, pattern: inner } => {
                self.store_local(pattern.span, *local, value);
                self.bind_irrefutable_pattern(inner, value, ty);
            }
            HirPatternKind::Struct { fields, .. } => {
                for field in fields {
                    let local_ty = field
                        .shorthand_local
                        .and_then(|id| self.typed.local_types.get(&id).cloned())
                        .or_else(|| {
                            field.pattern.as_ref().and_then(|p| first_bound_local(p)).and_then(|id| {
                                self.typed.local_types.get(&id).cloned()
                            })
                        })
                        .unwrap_or(Ty::Unknown);
                    let field_value = self.emit_value(
                        pattern.span,
                        local_ty.clone(),
                        FirInstructionKind::ExtractField {
                            base: value,
                            field: field.name.clone(),
                        },
                    );
                    if let Some(local) = field.shorthand_local {
                        self.store_local(pattern.span, local, field_value);
                    }
                    if let Some(inner) = &field.pattern {
                        self.bind_irrefutable_pattern(inner, field_value, &local_ty);
                    }
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let element = match ty {
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => element.as_ref().clone(),
                    _ => Ty::Unknown,
                };
                for (index, item) in items.iter().enumerate() {
                    let index_value = self.emit_value(
                        item.span,
                        Ty::Int {
                            signed: false,
                            width: IntWidth::Pointer,
                        },
                        FirInstructionKind::Const {
                            value: FirConst::Integer {
                                text: index.to_string(),
                            },
                        },
                    );
                    let item_value = self.emit_value(
                        item.span,
                        element.clone(),
                        FirInstructionKind::IndexUnchecked {
                            base: value,
                            index: index_value,
                        },
                    );
                    self.bind_irrefutable_pattern(item, item_value, &element);
                }
                if let Some(rest) = rest {
                    self.diagnostic(
                        pattern.span,
                        "fir/rest-pattern",
                        format!("irrefutable rest binding {rest:?} needs slice-view lowering"),
                    );
                }
            }
            HirPatternKind::Variant { fields, .. } => {
                for field in fields {
                    let field_ty = field
                        .shorthand_local
                        .and_then(|id| self.typed.local_types.get(&id).cloned())
                        .unwrap_or(Ty::Unknown);
                    let field_value = self.emit_value(
                        pattern.span,
                        field_ty.clone(),
                        FirInstructionKind::ExtractField {
                            base: value,
                            field: field.name.clone(),
                        },
                    );
                    if let Some(local) = field.shorthand_local {
                        self.store_local(pattern.span, local, field_value);
                    }
                    if let Some(inner) = &field.pattern {
                        self.bind_irrefutable_pattern(inner, field_value, &field_ty);
                    }
                }
            }
            other => self.diagnostic(
                pattern.span,
                "fir/refutable-binding",
                format!("refutable pattern reached irrefutable FIR binding: {other:?}"),
            ),
        }
    }

    fn store_local(&mut self, span: Span, source: LocalId, value: FirValueId) {
        if self.typed.local_constants.contains_key(&source) {
            return;
        }
        if let Some(local) = self.local_map.get(&source).copied() {
            self.emit_void(
                span,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    value,
                },
            );
        } else {
            self.diagnostic(span, "fir/local", format!("missing local mapping for {source:?}"));
        }
    }

    fn poison(&mut self, span: Span, ty: Ty) -> FirValueId {
        self.emit_value(span, ty, FirInstructionKind::Poison)
    }
}

fn first_bound_local(pattern: &HirPattern) -> Option<LocalId> {
    match &pattern.kind {
        HirPatternKind::Binding { local, .. } | HirPatternKind::As { local, .. } => Some(*local),
        HirPatternKind::Some { value } => first_bound_local(value),
        HirPatternKind::Sequence { items, rest } => items
            .iter()
            .find_map(first_bound_local)
            .or(*rest),
        _ => None,
    }
}

fn first_positional(args: &[HirCallArg]) -> Option<&HirExpr> {
    args.iter().find_map(|arg| match arg {
        HirCallArg::Positional { value } => Some(value),
        HirCallArg::Named { .. } => None,
    })
}

fn arg_value(arg: &HirCallArg) -> &HirExpr {
    match arg {
        HirCallArg::Positional { value } | HirCallArg::Named { value, .. } => value,
    }
}

fn binary_overflow(op: BinaryOp, mode: OverflowMode) -> Option<OverflowMode> {
    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => Some(mode),
        BinaryOp::Div | BinaryOp::Rem | BinaryOp::ShiftLeft | BinaryOp::ShiftRight => {
            Some(OverflowMode::Checked)
        }
        _ => None,
    }
}

fn fir_type_is_concrete(ty: &Ty) -> bool {
    match ty {
        Ty::Error | Ty::Unknown | Ty::IntLiteral | Ty::FloatLiteral | Ty::NoneLiteral => false,
        Ty::Pointer { inner, .. }
        | Ty::Reference { inner, .. }
        | Ty::Optional { inner } => fir_type_is_concrete(inner),
        Ty::Slice { element, .. } => fir_type_is_concrete(element),
        Ty::Array { element, length } => length.is_some() && fir_type_is_concrete(element),
        Ty::Result { ok, error } => fir_type_is_concrete(ok) && fir_type_is_concrete(error),
        Ty::Function { params, result, .. } | Ty::Closure { params, result } => {
            params.iter().all(fir_type_is_concrete) && fir_type_is_concrete(result)
        }
        _ => true,
    }
}

pub fn verify_fir_function(function: &FirFunction) -> Vec<FirDiagnostic> {
    let mut diagnostics = Vec::new();
    let block_count = function.blocks.len() as u32;
    let mut definitions = BTreeSet::new();

    if !fir_type_is_concrete(&function.return_type) {
        diagnostics.push(FirDiagnostic {
            span: Span::new(0, 0),
            code: "fir/verify-type".into(),
            message: format!("non-concrete function return type {:?}", function.return_type),
        });
    }
    for local in function.locals.values() {
        if !fir_type_is_concrete(&local.ty) {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-type".into(),
                message: format!("non-concrete local {:?}: {:?}", local.id, local.ty),
            });
        }
    }

    for block in &function.blocks {
        if block.terminator.is_none() {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-terminator".into(),
                message: format!("block {:?} has no terminator", block.id),
            });
        }
        for instruction in &block.instructions {
            if let Some(result) = instruction.result {
                if !definitions.insert(result) {
                    diagnostics.push(FirDiagnostic {
                        span: instruction.span,
                        code: "fir/verify-value".into(),
                        message: format!("value {result:?} is defined more than once"),
                    });
                }
                if !function.value_types.contains_key(&result) {
                    diagnostics.push(FirDiagnostic {
                        span: instruction.span,
                        code: "fir/verify-value".into(),
                        message: format!("value {result:?} has no type"),
                    });
                }
            }
        }
        if let Some(term) = &block.terminator {
            let targets: Vec<FirBlockId> = match term {
                FirTerminator::Goto { target } => vec![*target],
                FirTerminator::Branch {
                    then_block,
                    else_block,
                    ..
                } => vec![*then_block, *else_block],
                FirTerminator::Return { .. } | FirTerminator::Unreachable => Vec::new(),
            };
            for target in targets {
                if target.0 >= block_count {
                    diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-target".into(),
                        message: format!("block {:?} targets missing block {target:?}", block.id),
                    });
                }
            }
            if let FirTerminator::Branch { condition, .. } = term {
                if function.value_types.get(condition) != Some(&Ty::Bool) {
                    diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-branch".into(),
                        message: format!("branch condition {condition:?} is not bool"),
                    });
                }
            }
            if let FirTerminator::Return { value } = term {
                match (value, &function.return_type) {
                    (None, Ty::Void) => {}
                    (Some(value), expected) if function.value_types.get(value) == Some(expected) => {}
                    _ => diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-return".into(),
                        message: format!(
                            "return value {:?} does not match {:?}",
                            value, function.return_type
                        ),
                    }),
                }
            }
        }
    }
    diagnostics
}
'''
Path("crates/forge-frontend/src/fir_v1.rs").write_text(fir)


# ---------------------------------------------------------------------------
# Public API.
# ---------------------------------------------------------------------------
p = Path("crates/forge-frontend/src/lib.rs")
text = p.read_text()
text = text.replace(
    '#[path = "hir_v1.rs"]\npub mod hir;\n',
    '#[path = "hir_v1.rs"]\npub mod hir;\n#[path = "fir_v1.rs"]\npub mod fir;\n',
    1,
)
text = text.replace(
    '''    lower_resolved_bodies, BodyHirOutput, HirBody, HirExpr, HirExprKind, HirGlobalBody,
    HirLocalDecl, HirPattern, HirPatternKind, HirStmt, HirStmtKind, HirType, HirTypeKind,
};''',
    '''    lower_resolved_bodies, BodyHirOutput, ExprId, HirBody, HirExpr, HirExprKind,
    HirGlobalBody, HirLocalDecl, HirPattern, HirPatternKind, HirStmt, HirStmtKind, HirType,
    HirTypeKind,
};''',
    1,
)
text += '''\npub use fir::{
    lower_fir, verify_fir_function, FirBasicBlock, FirBlockId, FirConst, FirDiagnostic,
    FirFunction, FirGlobal, FirInstruction, FirInstructionKind, FirLocal, FirLocalId, FirModule,
    FirOutput, FirPlace, FirTerminator, FirUnaryOp, FirValueId, OverflowMode,
};\n'''
text = text.replace(
    '''    type_check_module, ConstValue, IntWidth, Ty, TypeCheckOutput, TypeDiagnostic, TypedBody,
    TypedExpr, TypedExprKind,
};''',
    '''    type_check_module, ConstValue, IntWidth, ResolvedReceiver, Ty, TypeCheckOutput,
    TypeDiagnostic, TypedBody, TypedExpr, TypedExprKind,
};''',
    1,
)
p.write_text(text)


# ---------------------------------------------------------------------------
# Tests. Keep this first FIR slice focused on invariants at the semantic/FIR
# boundary, explicit checks, CFG, ? propagation, optional promotion, methods,
# and defer. Unsupported high-level semantic gaps are tested as diagnostics.
# ---------------------------------------------------------------------------
tests = r'''use forge_frontend::{
    lower_fir, lower_module, lower_resolved_bodies, parse_source, type_check_module, BinaryOp,
    FirInstructionKind, FirTerminator, OverflowMode, Ty, TypedExprKind,
};

fn lower(source: &str) -> forge_frontend::FirOutput {
    let parsed = parse_source(source);
    assert!(parsed.diagnostics.is_empty(), "parse: {:?}", parsed.diagnostics);
    let ast = parsed.ast.expect("AST");
    let hir = lower_module(&ast);
    assert!(hir.diagnostics.is_empty(), "hir: {:?}", hir.diagnostics);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    assert!(bodies.diagnostics.is_empty(), "body hir: {:?}", bodies.diagnostics);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "typed: {:?}", typed.diagnostics);
    lower_fir(&bodies, &typed)
}

fn instructions(output: &forge_frontend::FirOutput) -> impl Iterator<Item = &FirInstructionKind> {
    output
        .module
        .functions
        .values()
        .flat_map(|f| f.blocks.iter())
        .flat_map(|b| b.instructions.iter())
        .map(|i| &i.kind)
}

#[test]
fn lowers_checked_arithmetic_and_cfg() {
    let output = lower(
        r#"
        module test.fir_arithmetic;
        fn add(a: u32, b: u32) -> u32 {
            var x: u32 = a + b;
            if (x > 10u32) { x = x - 1u32; }
            while (x < 20u32) { x = x + 1u32; }
            return x;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: BinaryOp::Add,
            overflow: Some(OverflowMode::Checked),
            ..
        }
    )));
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.len() >= 6);
    assert!(function.blocks.iter().all(|b| b.terminator.is_some()));
}

#[test]
fn overflow_metadata_selects_wrapping_fir_operation() {
    let output = lower(
        r#"
        module test.fir_wrap;
        @overflow(wrap)
        fn add(a: u32, b: u32) -> u32 { return a + b; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary {
            op: BinaryOp::Add,
            overflow: Some(OverflowMode::Wrapping),
            ..
        }
    )));
}

#[test]
fn indexing_has_explicit_bounds_check() {
    let output = lower(
        r#"
        module test.fir_bounds;
        fn read(values: [u32; 4], index: usize) -> u32 { return values[index]; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::BoundsCheck { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::IndexUnchecked { .. })));
}

#[test]
fn result_try_is_explicit_cfg_with_error_return() {
    let output = lower(
        r#"
        module test.fir_try;
        fn pass(value: Result[u32, u8]) -> Result[u32, u8] {
            value?;
            return value;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultIsOk { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::ResultUnwrapErr { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeResultErr { .. })));
    let function = output.module.functions.values().next().unwrap();
    assert!(function.blocks.iter().filter(|b| matches!(b.terminator, Some(FirTerminator::Return { .. }))).count() >= 2);
}

#[test]
fn optional_promotion_is_retained_and_lowered_to_some() {
    let parsed = parse_source(
        r#"
        module test.fir_optional;
        fn maybe(value: u32) -> u32? { return value; }
        "#,
    );
    assert!(parsed.diagnostics.is_empty());
    let ast = parsed.ast.unwrap();
    let hir = lower_module(&ast);
    let bodies = lower_resolved_bodies(&ast, &hir.module);
    let typed = type_check_module(&ast, &hir.module, &bodies);
    assert!(typed.diagnostics.is_empty(), "{:?}", typed.diagnostics);
    assert!(typed.functions.values().any(|body| body.expressions.iter().any(|expr| matches!(expr.kind, TypedExprKind::OptionalPromote { .. }))));
    let output = lower_fir(&bodies, &typed);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::MakeSome { .. })));
}

#[test]
fn method_reference_receiver_becomes_explicit_address() {
    let output = lower(
        r#"
        module test.fir_method;
        struct Point { x: i32; }
        impl Point {
            fn get(self: &Point) -> i32 { return self.x; }
        }
        fn main() -> i32 {
            val point = Point{x: 7i32};
            return point.get();
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::AddressOf { mutable: false, .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::Call { args, .. } if !args.is_empty())));
}

#[test]
fn defer_call_is_emitted_before_return() {
    let output = lower(
        r#"
        module test.fir_defer;
        fn cleanup() -> void { return; }
        fn main() -> i32 {
            defer cleanup();
            return 7i32;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let main = output.module.functions.values().find(|f| f.return_type == Ty::Int { signed: true, width: forge_frontend::IntWidth::W32 }).unwrap();
    let return_block = main.blocks.iter().find(|b| matches!(b.terminator, Some(FirTerminator::Return { .. }))).unwrap();
    assert!(return_block.instructions.iter().any(|i| matches!(i.kind, FirInstructionKind::Call { .. })));
}

#[test]
fn fir_reports_missing_pre_fir_pattern_decision_tree() {
    let output = lower(
        r#"
        module test.fir_match_gap;
        fn choose(value: bool) -> i32 {
            return match (value) { true => 1i32, false => 2i32, };
        }
        "#,
    );
    assert!(output.diagnostics.iter().any(|d| d.code == "fir/pattern-decision-tree-missing"));
}
'''
# BinaryOp is not currently re-exported from root, use ast path in the test.
tests = tests.replace('type_check_module, BinaryOp,\n', 'type_check_module,\n')
tests = tests.replace('BinaryOp::', 'forge_frontend::ast::BinaryOp::')
Path("crates/forge-frontend/tests/fir.rs").write_text(tests)


# ---------------------------------------------------------------------------
# Architecture docs: make the implemented boundary and known pre-FIR gaps
# explicit so later work does not push semantic reconstruction into FIR.
# ---------------------------------------------------------------------------
p = Path("docs/compiler-architecture.md")
text = p.read_text()
needle = '''IR should include:

- checked/wrapping arithmetic as distinct operations;'''
replacement = '''The bootstrap FIR is now implemented in `forge-frontend::fir`. Each HIR expression has a stable body-local `ExprId`, and typed HIR retains exact function signatures, resolved receiver transformations, and named-argument parameter indices. FIR lowering consumes those semantic facts directly; it never matches source spans or re-runs overload/type resolution. FIR includes a verifier that rejects missing terminators, invalid block targets, return-type mismatches, and non-concrete semantic types.

The first lowering slice covers literals, locals/globals, direct and indirect calls, method auto-reference, explicit conversions, aggregates, checked/wrapping arithmetic, short-circuit boolean control flow, safe indexing with explicit bounds checks, assignments/places, `if`, `while`, C-style `for`, `foreach`, `break`/`continue`, `defer`, optional promotion, and `Result` propagation through explicit success/error CFG edges.

FIR deliberately diagnoses rather than guesses when an earlier semantic stage is incomplete. In particular, pattern decision trees, closure-environment semantics, materialized default call arguments, typed context overrides, and typed channel/select operations must be completed before those constructs can cross the FIR boundary.

IR should include:

- checked/wrapping arithmetic as distinct operations;'''
if needle not in text:
    raise SystemExit("architecture FIR needle missing")
text = text.replace(needle, replacement, 1)
p.write_text(text)

p = Path("docs/frontend-ir.md")
text = p.read_text()
text = text.replace(
    '''FIR does **not** need to be SSA in the first compiler. SSA can later be constructed from FIR as an optimization representation.''',
    '''FIR does **not** need to be SSA in the first compiler. SSA can later be constructed from FIR as an optimization representation.

The bootstrap implementation uses body-local expression IDs to connect HIR occurrences to typed semantic facts. Function signatures and implicit method receiver transformations are retained in typed HIR, so FIR lowering is not permitted to reconstruct them from syntax. A FIR verifier enforces that concrete types, explicit terminators, valid CFG targets and return types survive the semantic boundary.''',
    1,
)
p.write_text(text)


# Add a typechecker regression for semantic facts FIR relies on.
p = Path("crates/forge-frontend/tests/typecheck.rs")
text = p.read_text()
addition = r'''

#[test]
fn typed_hir_retains_fir_boundary_facts() {
    let output = check(
        r#"
        module test.fir_boundary_facts;
        struct Point { x: u32; }
        impl Point { fn get(self: &Point) -> u32 { return self.x; } }
        nfn combine(left: u32, right: u32) -> u32 { return left + right; }
        fn maybe(point: Point) -> u32? {
            val x = combine(:right = 2u32, :left = point.get());
            return x;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let maybe = output
        .functions
        .values()
        .find(|body| matches!(body.return_type, Ty::Optional { .. }))
        .expect("maybe body");
    assert_eq!(maybe.params.len(), 1);
    assert!(maybe.expressions.iter().all(|expr| expr.id.0 < u32::MAX));
    assert!(maybe.expressions.iter().any(|expr| matches!(
        &expr.kind,
        forge_frontend::TypedExprKind::ResolvedCall {
            argument_parameters,
            ..
        } if argument_parameters == &vec![1, 0]
    )));
    assert!(maybe.expressions.iter().any(|expr| matches!(
        expr.kind,
        forge_frontend::TypedExprKind::OptionalPromote { .. }
    )));
}
'''
if "fn typed_hir_retains_fir_boundary_facts()" not in text:
    text += addition
p.write_text(text)

print("FIR implementation migration applied")
