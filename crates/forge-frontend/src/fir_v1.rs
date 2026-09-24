use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    ast::{BinaryOp, FdnValue, MetadataArg, Span, UnaryOp},
    body_hir::{
        BodyHirOutput, ExprId, HirBlock, HirBody, HirCallArg, HirExpr, HirExprKind, HirMatchBody,
        HirPattern, HirPatternKind, HirStmt, HirStmtKind,
    },
    hir::{DefId, MetadataTableExt, MetadataTarget},
    resolution::{LocalId, ResolvedBuiltinValue, ResolvedName},
    typecheck::{
        CaptureMode, ConstValue, ContextSlot, IntWidth, MatchCondition, MatchProjection,
        MatchScalar, MatchTest, ResolvedCallArgument, ResolvedReceiver, RuntimeOperationId, Ty,
        TypeCheckOutput, TypedBitFieldAccess, TypedBody, TypedClosurePlan, TypedExpr,
        TypedExprKind, TypedMatchBinding, TypedMatchPlan, TypedSelectArm, UnsafeOperationKind,
        UnsafeProvenance,
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

/// Target-owned protected-machine operations. These remain explicit in FIR so
/// generic backends cannot accidentally assign host-call semantics to them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sia32PrivilegedOperation {
    Trap { imm8: u8 },
    ReadSystem { system_register: u8 },
    WriteSystem { system_register: u8 },
    ReadGpr { register: u8 },
    WriteGpr { register: u8 },
    SwapScratch,
    Return,
    ReturnContext,
    TlbFence,
    TlbFenceVa,
    TlbFenceAsid,
    WaitForInterrupt,
    SyncInstruction,
    Fence,
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
    pub global_initializers: BTreeMap<DefId, FirGlobalInitializer>,
    pub global_init_order: Vec<DefId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirGlobal {
    pub owner: DefId,
    pub ty: Ty,
    pub mutable: bool,
    pub constant: Option<ConstValue>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirGlobalInitializer {
    pub owner: DefId,
    pub dependencies: Vec<DefId>,
    pub function: FirFunction,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirFunction {
    pub owner: DefId,
    pub params: Vec<FirLocalId>,
    pub return_type: Ty,
    pub locals: BTreeMap<FirLocalId, FirLocal>,
    pub closures: BTreeMap<ExprId, FirClosure>,
    pub entry: FirBlockId,
    pub blocks: Vec<FirBasicBlock>,
    pub value_types: BTreeMap<FirValueId, Ty>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirClosure {
    pub id: ExprId,
    pub captures: Vec<FirClosureField>,
    pub params: Vec<FirLocalId>,
    pub return_type: Ty,
    pub entry: FirBlockId,
    pub function_pointer: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FirClosureField {
    pub local: LocalId,
    pub ty: Ty,
    pub mode: CaptureMode,
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
    pub closure: Option<ExprId>,
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
#[serde(tag = "instruction", rename_all = "snake_case")]
pub enum FirInstructionKind {
    Const {
        value: FirConst,
    },
    Unit,
    FunctionRef {
        target: DefId,
    },
    LoadGlobal {
        global: DefId,
    },
    StoreGlobal {
        global: DefId,
        value: FirValueId,
    },
    AddressOfGlobal {
        global: DefId,
        mutable: bool,
    },
    ContextLoad {
        slot: ContextSlot,
    },
    ContextSave {
        slot: ContextSlot,
    },
    ContextSet {
        slot: ContextSlot,
        value: FirValueId,
    },
    ContextRestore {
        slot: ContextSlot,
        saved: FirValueId,
    },
    Load {
        place: FirPlace,
    },
    Store {
        place: FirPlace,
        value: FirValueId,
    },
    Unary {
        op: FirUnaryOp,
        value: FirValueId,
    },
    Binary {
        op: BinaryOp,
        overflow: Option<OverflowMode>,
        left: FirValueId,
        right: FirValueId,
    },
    Convert {
        value: FirValueId,
        target: Ty,
    },
    /// Convert an integer value to a floating-point value. This is separate
    /// from `Convert` because integer-to-float conversion can round and must
    /// not weaken the generic lossless-integer conversion contract.
    IntegerToFloat {
        value: FirValueId,
        target: Ty,
    },
    /// Convert between floating-point widths. This is separate from
    /// `Convert` because demotion can round and must not weaken the generic
    /// lossless-integer conversion contract.
    FloatConvert {
        value: FirValueId,
        target: Ty,
    },
    /// Form a non-owning slice view from an explicit reference to a fixed
    /// array. This is separate from `Convert`: it constructs the slice's
    /// pointer/length representation and must never relax numeric conversion
    /// rules.
    SliceFromArrayRef {
        value: FirValueId,
    },
    BitStructStorage {
        value: FirValueId,
        storage: Ty,
    },
    BitStructFromStorage {
        value: FirValueId,
        bitstruct: DefId,
    },
    /// Reinterpret a value with the declared underlying representation as a
    /// nominal `distinct` value. This is separate from `Convert` so it cannot
    /// relax ordinary numeric narrowing rules.
    DistinctFromUnderlying {
        value: FirValueId,
        distinct: DefId,
    },
    /// Extract a nominal `distinct` value's declared underlying
    /// representation. This is separate from `Convert` for the same reason as
    /// `DistinctFromUnderlying`.
    DistinctToUnderlying {
        value: FirValueId,
        distinct: DefId,
    },
    BitFieldCheck {
        value: FirValueId,
        width: u32,
    },
    /// Convert a masked storage value into its declared unsigned bit-field
    /// type. Unlike `Convert`, this is safe to reduce because the mask is a
    /// preceding Forge bit-field operation, not an arbitrary source cast.
    BitFieldExtract {
        value: FirValueId,
    },
    /// Zero-extend a checked numeric field into its declared bitstruct
    /// storage type. This is the inverse of `BitFieldExtract` for insertion.
    BitFieldExtend {
        value: FirValueId,
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
        items: Vec<FirValueId>,
    },
    MakeAggregate {
        ty: Ty,
        variant: Option<String>,
        fields: Vec<(String, FirValueId)>,
    },
    MakeNone,
    MakeSome {
        value: FirValueId,
    },
    Variant {
        ty: Ty,
        name: String,
    },
    VariantIs {
        value: FirValueId,
        name: String,
    },
    ExtractField {
        base: FirValueId,
        field: String,
    },
    Len {
        value: FirValueId,
    },
    BoundsCheck {
        index: FirValueId,
        len: FirValueId,
    },
    IndexUnchecked {
        base: FirValueId,
        index: FirValueId,
    },
    Subsequence {
        base: FirValueId,
        start: u64,
    },
    CollectionPatternLookup {
        collection: FirValueId,
        operation: DefId,
        key: FirValueId,
    },
    CollectionPatternHasOnly {
        collection: FirValueId,
        operation: DefId,
        keys: Vec<FirValueId>,
    },
    AddressOf {
        place: FirPlace,
        mutable: bool,
    },
    MakeClosure {
        closure: ExprId,
        captures: Vec<FirValueId>,
    },
    CallClosure {
        closure: FirValueId,
        args: Vec<FirValueId>,
        tail: bool,
    },
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
    /// SIA32-P operation retained as an explicit target operation through FIR.
    /// Operand values, when required, are carried in `args`.
    Sia32Privileged {
        operation: Sia32PrivilegedOperation,
        args: Vec<FirValueId>,
    },
    ResultIsOk {
        value: FirValueId,
    },
    ResultUnwrapOk {
        value: FirValueId,
    },
    ResultUnwrapErr {
        value: FirValueId,
    },
    MakeResultErr {
        error: FirValueId,
    },
    MakeResultOk {
        value: FirValueId,
    },
    OptionIsSome {
        value: FirValueId,
    },
    OptionUnwrap {
        value: FirValueId,
    },
    Poison,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "place", rename_all = "snake_case")]
pub enum FirPlace {
    Local {
        local: FirLocalId,
    },
    ClosureCapture {
        closure: ExprId,
        index: u32,
    },
    Field {
        base: Box<FirPlace>,
        field: String,
    },
    Index {
        base: Box<FirPlace>,
        index: FirValueId,
    },
    Deref {
        address: FirValueId,
    },
    RawDeref {
        address: FirValueId,
        volatile: bool,
        provenance: UnsafeProvenance,
    },
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
    Duration { value: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "case", rename_all = "snake_case")]
pub enum FirSelectCase {
    Receive {
        operation: RuntimeOperationId,
        channel: FirValueId,
        payload: FirLocalId,
        payload_type: Ty,
        target: FirBlockId,
    },
    Timeout {
        operation: RuntimeOperationId,
        duration: FirValueId,
        target: FirBlockId,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "term", rename_all = "snake_case")]
pub enum FirTerminator {
    Goto {
        target: FirBlockId,
    },
    Branch {
        condition: FirValueId,
        then_block: FirBlockId,
        else_block: FirBlockId,
    },
    Select {
        operation: RuntimeOperationId,
        cases: Vec<FirSelectCase>,
    },
    Return {
        value: Option<FirValueId>,
    },
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
                mutable: typed.mutable_globals.contains(owner),
                constant: typed.constants.get(owner).cloned(),
            },
        );
    }

    output.module.global_init_order = typed.global_init_order.clone();
    for owner in &typed.global_init_order {
        let Some(plan) = typed.global_initializers.get(owner) else {
            output.diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/global-init-plan".into(),
                message: format!("missing typed runtime global initializer for {owner:?}"),
            });
            continue;
        };
        let Some(global) = bodies.globals.get(owner) else {
            output.diagnostics.push(FirDiagnostic {
                span: plan.span,
                code: "fir/global-init-body".into(),
                message: format!("missing HIR runtime global initializer for {owner:?}"),
            });
            continue;
        };
        let synthetic = HirBody {
            owner: *owner,
            params: Vec::new(),
            param_defaults: BTreeMap::new(),
            return_type: global.ty.clone(),
            locals: global.locals.clone(),
            block: HirBlock {
                span: global.value.span,
                statements: vec![HirStmt {
                    span: global.value.span,
                    kind: HirStmtKind::Return {
                        tail: false,
                        value: Some(global.value.clone()),
                    },
                }],
            },
        };
        let (function, mut diagnostics) = FunctionLowerer::new(
            &synthetic,
            &plan.body,
            bodies,
            typed,
            function_overflow_mode(typed, *owner),
        )
        .lower();
        diagnostics.extend(verify_fir_function(&function));
        output.diagnostics.extend(diagnostics);
        output.module.global_initializers.insert(
            *owner,
            FirGlobalInitializer {
                owner: *owner,
                dependencies: plan.dependencies.clone(),
                function,
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
    ContextRestore {
        span: Span,
        slot: ContextSlot,
        saved: FirValueId,
    },
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
    all_typed: &'a TypeCheckOutput,
    exprs: BTreeMap<ExprId, &'a TypedExpr>,
    local_map: BTreeMap<LocalId, FirLocalId>,
    local_constants: BTreeMap<LocalId, ConstValue>,
    overflow: OverflowMode,
    function: FirFunction,
    current: FirBlockId,
    next_value: u32,
    next_local: u32,
    cleanup_scopes: Vec<Vec<Deferred>>,
    loops: Vec<LoopTargets>,
    diagnostics: Vec<FirDiagnostic>,
    in_cleanup: bool,
    active_closure: Option<ExprId>,
    active_return_type: Ty,
    closure_capture_places: BTreeMap<LocalId, (ExprId, u32)>,
}

impl<'a> FunctionLowerer<'a> {
    fn new(
        body: &'a crate::body_hir::HirBody,
        typed: &'a TypedBody,
        _bodies: &'a BodyHirOutput,
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
            closures: BTreeMap::new(),
            entry,
            blocks: vec![FirBasicBlock {
                id: entry,
                closure: None,
                instructions: Vec::new(),
                terminator: None,
            }],
            value_types: BTreeMap::new(),
        };
        let mut local_map = BTreeMap::new();
        let param_set = typed
            .params
            .iter()
            .map(|(id, _)| *id)
            .collect::<BTreeSet<_>>();
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
            all_typed,
            exprs,
            local_map,
            local_constants: typed.local_constants.clone(),
            overflow,
            function,
            current: entry,
            next_value: 0,
            next_local,
            cleanup_scopes: Vec::new(),
            loops: Vec::new(),
            diagnostics: Vec::new(),
            in_cleanup: false,
            active_closure: None,
            active_return_type: typed.return_type.clone(),
            closure_capture_places: BTreeMap::new(),
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
            closure: self.active_closure,
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
                if self.lower_bitfield_assignment(target, value) {
                    return;
                }
                let value = self.lower_expr(value);
                if let HirExprKind::Name { reference } = &target.kind {
                    if let ResolvedName::Def(global) = reference.root {
                        if self.all_typed.global_types.contains_key(&global) {
                            self.emit_void(
                                stmt.span,
                                FirInstructionKind::StoreGlobal { global, value },
                            );
                            return;
                        }
                    }
                }
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
            HirStmtKind::WithContext { overrides, body } => {
                self.lower_with_context(stmt.span, overrides, body)
            }
            HirStmtKind::Select { arms } => self.lower_select(stmt.span, arms),
        }
    }

    fn lower_with_context(&mut self, span: Span, overrides: &[(String, HirExpr)], body: &HirBlock) {
        let Some(plan) = self
            .typed
            .context_scopes
            .iter()
            .find(|scope| scope.span == span)
            .cloned()
        else {
            self.diagnostic(
                span,
                "fir/context-plan-missing",
                "typed HIR has no execution-context plan for this scope",
            );
            return;
        };
        if plan.overrides.len() != overrides.len() {
            self.diagnostic(
                span,
                "fir/context-plan-shape",
                "typed execution-context plan does not match source override count",
            );
        }

        self.cleanup_scopes.push(Vec::new());
        let cleanup_scope = self.cleanup_scopes.len() - 1;
        for (index, planned) in plan.overrides.iter().enumerate() {
            let Some((_, source)) = overrides.get(index) else {
                continue;
            };
            if planned.value != source.id {
                self.diagnostic(
                    source.span,
                    "fir/context-plan-shape",
                    "typed execution-context plan references a different override expression",
                );
            }
            let saved = self.emit_value(
                source.span,
                Ty::ContextSlot { slot: planned.slot },
                FirInstructionKind::ContextSave { slot: planned.slot },
            );
            let value = self.lower_expr(source);
            self.emit_void(
                source.span,
                FirInstructionKind::ContextSet {
                    slot: planned.slot,
                    value,
                },
            );
            if let Some(scope) = self.cleanup_scopes.get_mut(cleanup_scope) {
                scope.push(Deferred::ContextRestore {
                    span: source.span,
                    slot: planned.slot,
                    saved,
                });
            }
        }

        self.lower_block(body);
        if !self.terminated() {
            self.emit_scope_cleanups(cleanup_scope);
        }
        self.cleanup_scopes.pop();
    }

    fn lower_select(&mut self, span: Span, arms: &[crate::body_hir::HirSelectArm]) {
        let Some(plan) = self
            .typed
            .select_plans
            .iter()
            .find(|plan| plan.span == span)
            .cloned()
        else {
            self.diagnostic(
                span,
                "fir/select-plan",
                "select reached FIR without a typed select plan",
            );
            return;
        };
        if plan.arms.len() != arms.len() {
            self.diagnostic(
                span,
                "fir/select-plan",
                "typed select plan does not match source arm count",
            );
            return;
        }

        let join = self.new_block();
        let mut cases = Vec::with_capacity(arms.len());
        let mut lowered = Vec::with_capacity(arms.len());

        // Evaluate channel/timeout operands exactly once, in source-arm order, before waiting.
        for (arm, planned) in arms.iter().zip(&plan.arms) {
            let target = self.new_block();
            match (arm, planned) {
                (
                    crate::body_hir::HirSelectArm::Receive {
                        channel,
                        pattern,
                        body,
                    },
                    TypedSelectArm::Receive {
                        channel: planned_id,
                        payload,
                        operation,
                    },
                ) if channel.id == *planned_id => {
                    let channel_value = self.lower_expr(channel);
                    let payload_local = self.synthetic_local(payload.clone());
                    cases.push(FirSelectCase::Receive {
                        operation: *operation,
                        channel: channel_value,
                        payload: payload_local,
                        payload_type: payload.clone(),
                        target,
                    });
                    lowered.push((
                        target,
                        Some((payload_local, payload.clone(), pattern)),
                        body,
                    ));
                }
                (
                    crate::body_hir::HirSelectArm::Timeout { duration, body },
                    TypedSelectArm::Timeout {
                        duration: planned_id,
                        operation,
                    },
                ) if duration.id == *planned_id => {
                    let duration_value = self.lower_expr(duration);
                    cases.push(FirSelectCase::Timeout {
                        operation: *operation,
                        duration: duration_value,
                        target,
                    });
                    lowered.push((target, None, body));
                }
                _ => {
                    self.diagnostic(
                        span,
                        "fir/select-plan",
                        "typed select arm does not match source arm",
                    );
                    return;
                }
            }
        }

        self.terminate(FirTerminator::Select {
            operation: plan.operation,
            cases,
        });
        for (target, receive, body) in lowered {
            self.switch_to(target);
            if let Some((payload_local, payload_ty, pattern)) = receive {
                let payload = self.emit_value(
                    pattern.span,
                    payload_ty.clone(),
                    FirInstructionKind::Load {
                        place: FirPlace::Local {
                            local: payload_local,
                        },
                    },
                );
                self.bind_irrefutable_pattern(pattern, payload, &payload_ty);
            }
            self.lower_block(body);
            if !self.terminated() {
                self.terminate(FirTerminator::Goto { target: join });
            }
        }
        self.switch_to(join);
    }

    fn lower_if(
        &mut self,
        condition: &HirExpr,
        then_block: &HirBlock,
        else_stmt: Option<&HirStmt>,
    ) {
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
        let then_reaches_join = !self.terminated();
        if then_reaches_join {
            self.terminate(FirTerminator::Goto { target: join_id });
        }

        self.switch_to(else_id);
        if let Some(stmt) = else_stmt {
            self.lower_stmt(stmt);
        }
        let else_reaches_join = !self.terminated();
        if else_reaches_join {
            self.terminate(FirTerminator::Goto { target: join_id });
        }

        self.switch_to(join_id);
        if !then_reaches_join && !else_reaches_join {
            self.terminate(FirTerminator::Unreachable);
        }
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
                Deferred::ContextRestore { span, slot, saved } => {
                    self.emit_void(span, FirInstructionKind::ContextRestore { slot, saved })
                }
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
                arguments,
                ..
            } => self.lower_resolved_call(
                expr, *target, *method, *receiver, arguments, result_ty, false,
            ),
            TypedExprKind::ResolvedClosure { plan, .. } => {
                self.lower_closure(expr, plan, result_ty)
            }
            TypedExprKind::ResolvedContext { slot, .. } => self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::ContextLoad { slot: *slot },
            ),
            TypedExprKind::ResolvedTry {
                source_error,
                target_error,
                ..
            } => self.lower_try(expr, source_error, target_error, result_ty),
            TypedExprKind::ResolvedMatch { plan, .. } => self.lower_match(expr, plan, result_ty),
            TypedExprKind::ResolvedBitField { access, .. } => {
                self.lower_bitfield_read(expr, access, result_ty)
            }
            TypedExprKind::Sia32Privileged { operation, .. } => {
                let HirExprKind::Call { args, .. } = &expr.kind else {
                    self.diagnostic(
                        expr.span,
                        "fir/sia32-shape",
                        "resolved SIA32 operation is not a call",
                    );
                    return self.poison(expr.span, result_ty);
                };
                // Selector immediates for TRAP/SREAD/SWRITE are encoded in
                // the FIR operation itself, not carried as runtime operands.
                // SWRITE therefore retains only its second (u32 value) operand.
                let lowered_args = match operation {
                    ResolvedBuiltinValue::SiaTrap
                    | ResolvedBuiltinValue::SiaSread
                    | ResolvedBuiltinValue::SiaGprRead => Vec::new(),
                    ResolvedBuiltinValue::SiaSwrite | ResolvedBuiltinValue::SiaGprWrite => args
                        .get(1)
                        .map(|arg| vec![self.lower_expr(arg_value(arg))])
                        .unwrap_or_default(),
                    _ => args
                        .iter()
                        .map(|arg| self.lower_expr(arg_value(arg)))
                        .collect::<Vec<_>>(),
                };
                let op = match operation {
                    ResolvedBuiltinValue::SiaTrap => {
                        let imm8 = first_positional(args)
                            .map(|expr| self.sia_immediate_u8(expr, "SIA32 privileged immediate"))
                            .unwrap_or(0);
                        Sia32PrivilegedOperation::Trap { imm8 }
                    }
                    ResolvedBuiltinValue::SiaSread => {
                        let system_register = first_positional(args)
                            .map(|expr| self.sia_immediate_u8(expr, "SIA32 privileged immediate"))
                            .unwrap_or(0);
                        Sia32PrivilegedOperation::ReadSystem { system_register }
                    }
                    ResolvedBuiltinValue::SiaSwrite => {
                        let system_register = first_positional(args)
                            .map(|expr| self.sia_immediate_u8(expr, "SIA32 privileged immediate"))
                            .unwrap_or(0);
                        Sia32PrivilegedOperation::WriteSystem { system_register }
                    }
                    ResolvedBuiltinValue::SiaGprRead => {
                        let register = first_positional(args)
                            .map(|expr| self.sia_gpr_number(expr))
                            .unwrap_or(0);
                        Sia32PrivilegedOperation::ReadGpr { register }
                    }
                    ResolvedBuiltinValue::SiaGprWrite => {
                        let register = first_positional(args)
                            .map(|expr| self.sia_gpr_number(expr))
                            .unwrap_or(0);
                        Sia32PrivilegedOperation::WriteGpr { register }
                    }
                    ResolvedBuiltinValue::SiaSswapScratch => Sia32PrivilegedOperation::SwapScratch,
                    ResolvedBuiltinValue::SiaSret => Sia32PrivilegedOperation::Return,
                    ResolvedBuiltinValue::SiaSretctx => Sia32PrivilegedOperation::ReturnContext,
                    ResolvedBuiltinValue::SiaTlbfence => Sia32PrivilegedOperation::TlbFence,
                    ResolvedBuiltinValue::SiaTlbfenceVa => Sia32PrivilegedOperation::TlbFenceVa,
                    ResolvedBuiltinValue::SiaTlbfenceAsid => Sia32PrivilegedOperation::TlbFenceAsid,
                    ResolvedBuiltinValue::SiaWfi => Sia32PrivilegedOperation::WaitForInterrupt,
                    ResolvedBuiltinValue::SiaSyncI => Sia32PrivilegedOperation::SyncInstruction,
                    ResolvedBuiltinValue::SiaFence => Sia32PrivilegedOperation::Fence,
                    _ => unreachable!(),
                };
                let instruction = FirInstructionKind::Sia32Privileged {
                    operation: op,
                    args: lowered_args,
                };
                if result_ty == Ty::Void || result_ty == Ty::Never {
                    self.emit_void(expr.span, instruction);
                    // Expression statements still require an internal value id from
                    // lower_expr; keep that bookkeeping separate from the void
                    // privileged instruction itself.
                    self.emit_value(expr.span, Ty::Void, FirInstructionKind::Unit)
                } else {
                    self.emit_value(expr.span, result_ty, instruction)
                }
            }
            TypedExprKind::BuiltinConstructor { constructor, .. } => {
                let HirExprKind::Call { args, .. } = &expr.kind else {
                    self.diagnostic(
                        expr.span,
                        "fir/constructor-shape",
                        "resolved constructor is not a call",
                    );
                    return self.poison(expr.span, result_ty);
                };
                let value = if let Some(value) = first_positional(args) {
                    self.lower_expr(value)
                } else if args.is_empty()
                    && match (constructor, &result_ty) {
                        (ResolvedBuiltinValue::Ok, Ty::Result { ok, .. }) => **ok == Ty::Void,
                        (ResolvedBuiltinValue::Err, Ty::Result { error, .. }) => {
                            **error == Ty::Void
                        }
                        _ => false,
                    }
                {
                    self.emit_value(expr.span, Ty::Void, FirInstructionKind::Unit)
                } else {
                    self.diagnostic(
                        expr.span,
                        "fir/constructor-arguments",
                        "resolved constructor lacks a positional payload",
                    );
                    return self.poison(expr.span, result_ty);
                };
                match constructor {
                    ResolvedBuiltinValue::Some => self.emit_value(
                        expr.span,
                        result_ty,
                        FirInstructionKind::MakeSome { value },
                    ),
                    ResolvedBuiltinValue::Ok => self.emit_value(
                        expr.span,
                        result_ty,
                        FirInstructionKind::MakeResultOk { value },
                    ),
                    ResolvedBuiltinValue::Err => self.emit_value(
                        expr.span,
                        result_ty,
                        FirInstructionKind::MakeResultErr { error: value },
                    ),
                    _ => {
                        unreachable!("SIA32 builtins are lowered by TypedExprKind::Sia32Privileged")
                    }
                }
            }
            TypedExprKind::UnsafeOperation {
                operation,
                provenance,
                ..
            } => self.lower_unsafe_expr(expr, *operation, *provenance, result_ty),
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
            HirExprKind::Context { .. } => {
                self.diagnostic(
                    expr.span,
                    "fir/context-not-resolved",
                    "context slot reached FIR without semantic slot resolution",
                );
                self.poison(expr.span, ty)
            }
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
                let value = self.lower_expr(source_expr);
                if matches!(ty, Ty::Slice { .. })
                    && matches!(source_ty, Ty::Reference { ref inner, .. } if matches!(inner.as_ref(), Ty::Array { .. }))
                {
                    return self.emit_value(
                        expr.span,
                        ty,
                        FirInstructionKind::SliceFromArrayRef { value },
                    );
                }
                if let Ty::Nominal(id) = ty {
                    if self.all_typed.bitstructs.contains_key(&id) {
                        return self.emit_value(
                            expr.span,
                            Ty::Nominal(id),
                            FirInstructionKind::BitStructFromStorage {
                                value,
                                bitstruct: id,
                            },
                        );
                    }
                    return self.emit_value(
                        expr.span,
                        Ty::Nominal(id),
                        FirInstructionKind::DistinctFromUnderlying {
                            value,
                            distinct: id,
                        },
                    );
                }
                if let Ty::Nominal(id) = source_ty {
                    if !self.all_typed.bitstructs.contains_key(&id) {
                        return self.emit_value(
                            expr.span,
                            ty.clone(),
                            FirInstructionKind::DistinctToUnderlying {
                                value,
                                distinct: id,
                            },
                        );
                    }
                }
                if matches!(source_ty, Ty::Int { .. } | Ty::Byte) && matches!(ty, Ty::Float { .. })
                {
                    return self.emit_value(
                        expr.span,
                        ty.clone(),
                        FirInstructionKind::IntegerToFloat { value, target: ty },
                    );
                }
                if matches!(source_ty, Ty::Float { .. }) && matches!(ty, Ty::Float { .. }) {
                    return self.emit_value(
                        expr.span,
                        ty.clone(),
                        FirInstructionKind::FloatConvert { value, target: ty },
                    );
                }
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
                let base_ty = self.expr_ty(base);
                if name == "len" && matches!(base_ty, Ty::Array { .. } | Ty::Slice { .. } | Ty::Str)
                {
                    let base = self.lower_expr(base);
                    return self.emit_len(expr.span, base, &base_ty);
                }
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
                    "fir/unresolved-closure",
                    "closure reached FIR without a typed closure environment plan",
                );
                self.poison(expr.span, ty)
            }
            HirExprKind::ReaderForm { tag, value } if ty == Ty::Duration && tag == "duration" => {
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

    fn lower_unsafe_expr(
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
        plan: &TypedClosurePlan,
        result_ty: Ty,
    ) -> FirValueId {
        let HirExprKind::Closure { body, .. } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/closure-shape",
                "resolved closure plan is not attached to a closure HIR node",
            );
            return self.poison(expr.span, result_ty);
        };

        // Evaluate environment construction in the enclosing lexical context.
        let mut capture_values = Vec::with_capacity(plan.captures.len());
        for capture in &plan.captures {
            let value = match capture.mode {
                CaptureMode::Value => {
                    self.lower_name(expr.span, capture.source, capture.ty.clone())
                }
                CaptureMode::SharedReference | CaptureMode::MutableReference => {
                    let ResolvedName::Local(source) = capture.source else {
                        self.diagnostic(
                            expr.span,
                            "fir/closure-reference-source",
                            "reference closure capture has no local source place",
                        );
                        capture_values.push(self.poison(expr.span, Ty::Error));
                        continue;
                    };
                    let Some(place) = self.place_for_local(source) else {
                        self.diagnostic(
                            expr.span,
                            "fir/closure-reference-source",
                            "reference closure capture source has no FIR place",
                        );
                        capture_values.push(self.poison(expr.span, Ty::Error));
                        continue;
                    };
                    let mutable = capture.mode == CaptureMode::MutableReference;
                    let reference_ty = Ty::Reference {
                        mutable,
                        inner: Box::new(capture.ty.clone()),
                    };
                    self.emit_value(
                        expr.span,
                        reference_ty,
                        FirInstructionKind::AddressOf { place, mutable },
                    )
                }
            };
            capture_values.push(value);
        }

        let saved_current = self.current;
        let saved_active = self.active_closure;
        let saved_return = self.active_return_type.clone();
        let saved_capture_places = std::mem::take(&mut self.closure_capture_places);
        let saved_cleanups = std::mem::take(&mut self.cleanup_scopes);
        let saved_loops = std::mem::take(&mut self.loops);
        let saved_in_cleanup = self.in_cleanup;

        self.active_closure = Some(expr.id);
        self.active_return_type = plan.result.clone();
        self.in_cleanup = false;
        for (index, capture) in plan.captures.iter().enumerate() {
            self.closure_capture_places
                .insert(capture.local, (expr.id, index as u32));
        }
        let entry = self.new_block();
        self.switch_to(entry);
        self.lower_block(body);
        if !self.terminated() {
            if plan.result == Ty::Void {
                self.terminate(FirTerminator::Return { value: None });
            } else {
                self.diagnostic(
                    body.span,
                    "fir/closure-missing-return",
                    "control reaches the end of a non-void closure",
                );
                self.terminate(FirTerminator::Unreachable);
            }
        }

        let params = plan
            .params
            .iter()
            .filter_map(|(source, _)| self.local_map.get(source).copied())
            .collect::<Vec<_>>();
        if params.len() != plan.params.len() {
            self.diagnostic(
                expr.span,
                "fir/closure-parameter-local",
                "typed closure parameter has no FIR local",
            );
        }
        self.function.closures.insert(
            expr.id,
            FirClosure {
                id: expr.id,
                captures: plan
                    .captures
                    .iter()
                    .map(|capture| FirClosureField {
                        local: capture.local,
                        ty: capture.ty.clone(),
                        mode: capture.mode,
                    })
                    .collect(),
                params,
                return_type: plan.result.clone(),
                entry,
                function_pointer: plan.function_pointer,
            },
        );

        self.current = saved_current;
        self.active_closure = saved_active;
        self.active_return_type = saved_return;
        self.closure_capture_places = saved_capture_places;
        self.cleanup_scopes = saved_cleanups;
        self.loops = saved_loops;
        self.in_cleanup = saved_in_cleanup;

        self.emit_value(
            expr.span,
            result_ty,
            FirInstructionKind::MakeClosure {
                closure: expr.id,
                captures: capture_values,
            },
        )
    }

    fn place_for_local(&self, local: LocalId) -> Option<FirPlace> {
        if let Some((closure, index)) = self.closure_capture_places.get(&local).copied() {
            Some(FirPlace::ClosureCapture { closure, index })
        } else {
            self.local_map
                .get(&local)
                .copied()
                .map(|local| FirPlace::Local { local })
        }
    }

    fn lower_bitfield_read(
        &mut self,
        expr: &HirExpr,
        access: &TypedBitFieldAccess,
        result_ty: Ty,
    ) -> FirValueId {
        let HirExprKind::Member { base, .. } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/bitfield-shape",
                "resolved bit-field access is not attached to a member expression",
            );
            return self.poison(expr.span, result_ty);
        };
        let base_value = if let Some(place) = self.try_place(base) {
            self.emit_value(
                base.span,
                Ty::Nominal(access.owner),
                FirInstructionKind::Load { place },
            )
        } else {
            self.lower_expr(base)
        };
        let storage = self.emit_value(
            expr.span,
            access.storage.clone(),
            FirInstructionKind::BitStructStorage {
                value: base_value,
                storage: access.storage.clone(),
            },
        );
        let shifted = if access.field.offset == 0 {
            storage
        } else {
            let shift = self.emit_integer_const(
                expr.span,
                access.storage.clone(),
                access.field.offset as u64,
            );
            self.emit_value(
                expr.span,
                access.storage.clone(),
                FirInstructionKind::Binary {
                    op: BinaryOp::ShiftRight,
                    overflow: Some(OverflowMode::Checked),
                    left: storage,
                    right: shift,
                },
            )
        };
        let mask = bit_mask(access.field.width);
        let mask_value = self.emit_integer_const(expr.span, access.storage.clone(), mask);
        let masked = self.emit_value(
            expr.span,
            access.storage.clone(),
            FirInstructionKind::Binary {
                op: BinaryOp::BitAnd,
                overflow: None,
                left: shifted,
                right: mask_value,
            },
        );
        if result_ty == Ty::Bool {
            let zero = self.emit_integer_const(expr.span, access.storage.clone(), 0);
            self.emit_value(
                expr.span,
                Ty::Bool,
                FirInstructionKind::Binary {
                    op: BinaryOp::NotEq,
                    overflow: None,
                    left: masked,
                    right: zero,
                },
            )
        } else if result_ty == access.storage {
            masked
        } else {
            self.emit_value(
                expr.span,
                result_ty.clone(),
                FirInstructionKind::BitFieldExtract { value: masked },
            )
        }
    }

    fn lower_bitfield_assignment(&mut self, target: &HirExpr, value: &HirExpr) -> bool {
        let Some(typed) = self.exprs.get(&target.id).copied() else {
            return false;
        };
        let TypedExprKind::ResolvedBitField { access, .. } = &typed.kind else {
            return false;
        };
        let access = access.clone();
        let HirExprKind::Member { base, .. } = &target.kind else {
            return false;
        };
        let Some(base_place) = self.lower_place(base) else {
            return true;
        };
        let current = self.emit_value(
            base.span,
            Ty::Nominal(access.owner),
            FirInstructionKind::Load {
                place: base_place.clone(),
            },
        );
        let storage = self.emit_value(
            target.span,
            access.storage.clone(),
            FirInstructionKind::BitStructStorage {
                value: current,
                storage: access.storage.clone(),
            },
        );
        let field_value = self.lower_expr(value);
        if access.field.ty != Ty::Bool
            && access.field.width
                < integer_width_bits(&access.field.ty).unwrap_or(access.field.width)
        {
            self.emit_void(
                value.span,
                FirInstructionKind::BitFieldCheck {
                    value: field_value,
                    width: access.field.width,
                },
            );
        }
        let encoded = if access.field.ty == access.storage {
            field_value
        } else {
            self.emit_value(
                value.span,
                access.storage.clone(),
                FirInstructionKind::BitFieldExtend { value: field_value },
            )
        };
        let mask = bit_mask(access.field.width);
        let mask_value = self.emit_integer_const(target.span, access.storage.clone(), mask);
        let masked = self.emit_value(
            target.span,
            access.storage.clone(),
            FirInstructionKind::Binary {
                op: BinaryOp::BitAnd,
                overflow: None,
                left: encoded,
                right: mask_value,
            },
        );
        let shifted_value = if access.field.offset == 0 {
            masked
        } else {
            let shift = self.emit_integer_const(
                target.span,
                access.storage.clone(),
                access.field.offset as u64,
            );
            self.emit_value(
                target.span,
                access.storage.clone(),
                FirInstructionKind::Binary {
                    op: BinaryOp::ShiftLeft,
                    overflow: Some(OverflowMode::Checked),
                    left: masked,
                    right: shift,
                },
            )
        };
        let shifted_mask = mask << access.field.offset;
        let storage_mask = bit_mask(access.storage_bits);
        let clear_mask = storage_mask ^ shifted_mask;
        let clear_value = self.emit_integer_const(target.span, access.storage.clone(), clear_mask);
        let cleared = self.emit_value(
            target.span,
            access.storage.clone(),
            FirInstructionKind::Binary {
                op: BinaryOp::BitAnd,
                overflow: None,
                left: storage,
                right: clear_value,
            },
        );
        let combined = self.emit_value(
            target.span,
            access.storage.clone(),
            FirInstructionKind::Binary {
                op: BinaryOp::BitOr,
                overflow: None,
                left: cleared,
                right: shifted_value,
            },
        );
        let rebuilt = self.emit_value(
            target.span,
            Ty::Nominal(access.owner),
            FirInstructionKind::BitStructFromStorage {
                value: combined,
                bitstruct: access.owner,
            },
        );
        self.emit_void(
            target.span,
            FirInstructionKind::Store {
                place: base_place,
                value: rebuilt,
            },
        );
        true
    }

    fn emit_integer_const(&mut self, span: Span, ty: Ty, value: u64) -> FirValueId {
        self.emit_value(
            span,
            ty,
            FirInstructionKind::Const {
                value: FirConst::Integer {
                    text: value.to_string(),
                },
            },
        )
    }

    fn lower_match(&mut self, expr: &HirExpr, plan: &TypedMatchPlan, result_ty: Ty) -> FirValueId {
        let HirExprKind::Match { value, arms } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/match-shape",
                "resolved match plan is not attached to a match HIR node",
            );
            return self.poison(expr.span, result_ty);
        };
        let actual_scrutinee_type = self.expr_ty(value);
        if plan.scrutinee_type != actual_scrutinee_type || plan.arms.len() != arms.len() {
            self.diagnostic(
                expr.span,
                "fir/match-plan",
                "typed match plan does not match the source match shape/type",
            );
            return self.poison(expr.span, result_ty);
        }

        // Forge evaluation order requires the scrutinee to execute exactly once.
        let scrutinee = self.lower_expr(value);
        let result_local = if result_ty == Ty::Void {
            None
        } else {
            Some(self.synthetic_local(result_ty.clone()))
        };
        let join = self.new_block();

        for (arm, planned) in arms.iter().zip(&plan.arms) {
            let next_arm = self.new_block();
            let matched_entry = self.new_block();
            if planned.alternatives.is_empty() {
                self.terminate(FirTerminator::Goto { target: next_arm });
            } else {
                for (index, alternative) in planned.alternatives.iter().enumerate() {
                    let binding_entry = self.new_block();
                    let false_target = if index + 1 == planned.alternatives.len() {
                        next_arm
                    } else {
                        self.new_block()
                    };
                    let condition = self.lower_match_condition(
                        arm.pattern.span,
                        scrutinee,
                        &plan.scrutinee_type,
                        &alternative.condition,
                    );
                    if let Some(condition) = condition {
                        self.terminate(FirTerminator::Branch {
                            condition,
                            then_block: binding_entry,
                            else_block: false_target,
                        });
                    } else {
                        self.terminate(FirTerminator::Goto {
                            target: binding_entry,
                        });
                    }

                    self.switch_to(binding_entry);
                    self.lower_match_bindings(arm.pattern.span, scrutinee, &alternative.bindings);
                    if !self.terminated() {
                        self.terminate(FirTerminator::Goto {
                            target: matched_entry,
                        });
                    }
                    if index + 1 != planned.alternatives.len() {
                        self.switch_to(false_target);
                    }
                }
            }

            self.switch_to(matched_entry);
            if let Some(guard) = &arm.guard {
                let guard_value = self.lower_expr(guard);
                let body_entry = self.new_block();
                self.terminate(FirTerminator::Branch {
                    condition: guard_value,
                    then_block: body_entry,
                    else_block: next_arm,
                });
                self.switch_to(body_entry);
            }

            match &arm.body {
                HirMatchBody::Expr(body) => {
                    let value = self.lower_expr(body);
                    if let Some(local) = result_local {
                        self.emit_void(
                            body.span,
                            FirInstructionKind::Store {
                                place: FirPlace::Local { local },
                                value,
                            },
                        );
                    }
                }
                HirMatchBody::Block(body) => self.lower_block(body),
            }
            if !self.terminated() {
                self.terminate(FirTerminator::Goto { target: join });
            }
            self.switch_to(next_arm);
        }

        // Semantic exhaustiveness guarantees that valid finite matches cannot
        // fall through all unguarded coverage. Keep this explicit in FIR so a
        // broken semantic plan cannot become target-dependent behavior.
        if !self.terminated() {
            self.terminate(FirTerminator::Unreachable);
        }
        self.switch_to(join);
        if let Some(local) = result_local {
            self.emit_value(
                expr.span,
                result_ty,
                FirInstructionKind::Load {
                    place: FirPlace::Local { local },
                },
            )
        } else {
            self.emit_value(expr.span, Ty::Void, FirInstructionKind::Unit)
        }
    }

    fn lower_match_condition(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        scrutinee_ty: &Ty,
        condition: &MatchCondition,
    ) -> Option<FirValueId> {
        match condition {
            MatchCondition::Always => None,
            MatchCondition::Never => Some(self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::Const {
                    value: FirConst::Bool { value: false },
                },
            )),
            MatchCondition::Test { projections, test } => {
                let value = self.lower_match_projection(span, scrutinee, projections);
                let value_ty = projections
                    .last()
                    .map(|projection| match projection {
                        MatchProjection::OptionPayload { ty }
                        | MatchProjection::ResultPayload { ty, .. }
                        | MatchProjection::Field { ty, .. }
                        | MatchProjection::Index { ty, .. }
                        | MatchProjection::Rest { ty, .. }
                        | MatchProjection::CollectionLookup { ty, .. } => ty.clone(),
                    })
                    .unwrap_or_else(|| scrutinee_ty.clone());
                Some(self.lower_match_test(span, value, &value_ty, test))
            }
            MatchCondition::All { conditions } => {
                self.lower_match_condition_list(span, scrutinee, scrutinee_ty, conditions, true)
            }
            MatchCondition::Any { conditions } => {
                self.lower_match_condition_list(span, scrutinee, scrutinee_ty, conditions, false)
            }
        }
    }

    fn lower_match_condition_list(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        scrutinee_ty: &Ty,
        conditions: &[MatchCondition],
        all: bool,
    ) -> Option<FirValueId> {
        let mut terminal_block = None;
        let mut join_block = None;
        let mut result_local = None;
        let mut saw_test = false;

        for condition in conditions {
            let condition = self.lower_match_condition(span, scrutinee, scrutinee_ty, condition);
            let Some(condition) = condition else {
                if all {
                    continue;
                }
                return None;
            };
            saw_test = true;
            let terminal = *terminal_block.get_or_insert_with(|| self.new_block());
            let join = *join_block.get_or_insert_with(|| self.new_block());
            let local = *result_local.get_or_insert_with(|| self.synthetic_local(Ty::Bool));
            let next = self.new_block();
            self.terminate(if all {
                FirTerminator::Branch {
                    condition,
                    then_block: next,
                    else_block: terminal,
                }
            } else {
                FirTerminator::Branch {
                    condition,
                    then_block: terminal,
                    else_block: next,
                }
            });
            self.switch_to(next);
            let _ = (join, local);
        }

        if !saw_test {
            return if all {
                None
            } else {
                Some(self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Const {
                        value: FirConst::Bool { value: false },
                    },
                ))
            };
        }

        let terminal = terminal_block.expect("match condition terminal block");
        let join = join_block.expect("match condition join block");
        let local = result_local.expect("match condition result local");
        let fallthrough_value = self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Const {
                value: FirConst::Bool { value: all },
            },
        );
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local },
                value: fallthrough_value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });

        self.switch_to(terminal);
        let terminal_value = self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Const {
                value: FirConst::Bool { value: !all },
            },
        );
        self.emit_void(
            span,
            FirInstructionKind::Store {
                place: FirPlace::Local { local },
                value: terminal_value,
            },
        );
        self.terminate(FirTerminator::Goto { target: join });

        self.switch_to(join);
        Some(self.emit_value(
            span,
            Ty::Bool,
            FirInstructionKind::Load {
                place: FirPlace::Local { local },
            },
        ))
    }

    fn lower_match_test(
        &mut self,
        span: Span,
        value: FirValueId,
        value_ty: &Ty,
        test: &MatchTest,
    ) -> FirValueId {
        match test {
            MatchTest::Bool { value: true } => value,
            MatchTest::Bool { value: false } => self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::Unary {
                    op: FirUnaryOp::Not,
                    value,
                },
            ),
            MatchTest::ScalarLiteral { value: literal } => {
                let literal = self.emit_match_scalar(span, value_ty.clone(), literal);
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: BinaryOp::Eq,
                        overflow: None,
                        left: value,
                        right: literal,
                    },
                )
            }
            MatchTest::ScalarRange {
                start,
                end,
                inclusive,
            } => {
                let start = self.emit_match_scalar(span, value_ty.clone(), start);
                let end = self.emit_match_scalar(span, value_ty.clone(), end);
                let lower = self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: BinaryOp::GreaterEq,
                        overflow: None,
                        left: value,
                        right: start,
                    },
                );
                let upper = self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: if *inclusive {
                            BinaryOp::LessEq
                        } else {
                            BinaryOp::Less
                        },
                        overflow: None,
                        left: value,
                        right: end,
                    },
                );
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: BinaryOp::LogicalAnd,
                        overflow: None,
                        left: lower,
                        right: upper,
                    },
                )
            }
            MatchTest::OptionSome => {
                self.emit_value(span, Ty::Bool, FirInstructionKind::OptionIsSome { value })
            }
            MatchTest::OptionNone => {
                let is_some =
                    self.emit_value(span, Ty::Bool, FirInstructionKind::OptionIsSome { value });
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Unary {
                        op: FirUnaryOp::Not,
                        value: is_some,
                    },
                )
            }
            MatchTest::ResultOk => {
                self.emit_value(span, Ty::Bool, FirInstructionKind::ResultIsOk { value })
            }
            MatchTest::ResultErr => {
                let ok = self.emit_value(span, Ty::Bool, FirInstructionKind::ResultIsOk { value });
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Unary {
                        op: FirUnaryOp::Not,
                        value: ok,
                    },
                )
            }
            MatchTest::Variant { name } => self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::VariantIs {
                    value,
                    name: name.clone(),
                },
            ),
            MatchTest::CollectionHasOnly { operation, keys } => {
                let keys = keys
                    .iter()
                    .map(|key| {
                        self.emit_value(
                            span,
                            Ty::Str,
                            FirInstructionKind::Const {
                                value: FirConst::String { value: key.clone() },
                            },
                        )
                    })
                    .collect();
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::CollectionPatternHasOnly {
                        collection: value,
                        operation: *operation,
                        keys,
                    },
                )
            }
            MatchTest::Length { count, at_least } => {
                let len = self.emit_value(span, usize_ty(), FirInstructionKind::Len { value });
                let expected = self.emit_value(
                    span,
                    usize_ty(),
                    FirInstructionKind::Const {
                        value: FirConst::Integer {
                            text: count.to_string(),
                        },
                    },
                );
                self.emit_value(
                    span,
                    Ty::Bool,
                    FirInstructionKind::Binary {
                        op: if *at_least {
                            BinaryOp::GreaterEq
                        } else {
                            BinaryOp::Eq
                        },
                        overflow: None,
                        left: len,
                        right: expected,
                    },
                )
            }
        }
    }

    fn emit_match_scalar(&mut self, span: Span, ty: Ty, value: &MatchScalar) -> FirValueId {
        let value = match value {
            MatchScalar::Integer { text } => FirConst::Integer { text: text.clone() },
            MatchScalar::Character { value } => FirConst::Char { value: *value },
            MatchScalar::String { value } => FirConst::String {
                value: value.clone(),
            },
        };
        self.emit_value(span, ty, FirInstructionKind::Const { value })
    }

    fn lower_match_projection(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        projections: &[MatchProjection],
    ) -> FirValueId {
        let mut value = scrutinee;
        for projection in projections {
            value = match projection {
                MatchProjection::OptionPayload { ty } => {
                    self.emit_value(span, ty.clone(), FirInstructionKind::OptionUnwrap { value })
                }
                MatchProjection::ResultPayload { ty, ok } => self.emit_value(
                    span,
                    ty.clone(),
                    if *ok {
                        FirInstructionKind::ResultUnwrapOk { value }
                    } else {
                        FirInstructionKind::ResultUnwrapErr { value }
                    },
                ),
                MatchProjection::Field { name, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::ExtractField {
                        base: value,
                        field: name.clone(),
                    },
                ),
                MatchProjection::Index { index, ty } => {
                    let index = self.emit_value(
                        span,
                        usize_ty(),
                        FirInstructionKind::Const {
                            value: FirConst::Integer {
                                text: index.to_string(),
                            },
                        },
                    );
                    self.emit_value(
                        span,
                        ty.clone(),
                        FirInstructionKind::IndexUnchecked { base: value, index },
                    )
                }
                MatchProjection::Rest { start, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::Subsequence {
                        base: value,
                        start: *start,
                    },
                ),
                MatchProjection::CollectionLookup { operation, key, ty } => {
                    let key = self.emit_value(
                        span,
                        Ty::Str,
                        FirInstructionKind::Const {
                            value: FirConst::String { value: key.clone() },
                        },
                    );
                    self.emit_value(
                        span,
                        ty.clone(),
                        FirInstructionKind::CollectionPatternLookup {
                            collection: value,
                            operation: *operation,
                            key,
                        },
                    )
                }
            };
        }
        value
    }

    fn lower_match_bindings(
        &mut self,
        span: Span,
        scrutinee: FirValueId,
        bindings: &[TypedMatchBinding],
    ) {
        for binding in bindings {
            let value = self.lower_match_projection(span, scrutinee, &binding.projections);
            let Some(local) = self.local_map.get(&binding.local).copied() else {
                self.diagnostic(
                    span,
                    "fir/match-binding-local",
                    format!("match binding {:?} has no FIR local", binding.local),
                );
                continue;
            };
            self.emit_void(
                span,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    value,
                },
            );
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
                arguments,
                ..
            } => {
                self.lower_resolved_call(expr, target, method, receiver, &arguments, typed.ty, tail)
            }
            TypedExprKind::OptionalPromote {
                inner, source_type, ..
            } => {
                let value = self.lower_expr_kind(expr, &inner, source_type);
                self.emit_value(expr.span, typed.ty, FirInstructionKind::MakeSome { value })
            }
            _ => {
                let HirExprKind::Call { callee, args } = &expr.kind else {
                    return self.lower_expr(expr);
                };
                if args
                    .iter()
                    .any(|arg| matches!(arg, HirCallArg::Named { .. }))
                {
                    self.diagnostic(
                        expr.span,
                        "fir/indirect-named-call",
                        "named arguments on indirect calls need semantic parameter mapping",
                    );
                }
                let callee_ty = self.expr_ty(callee);
                let callee = self.lower_expr(callee);
                let args = args
                    .iter()
                    .map(arg_value)
                    .map(|arg| self.lower_expr(arg))
                    .collect();
                let kind = if matches!(callee_ty, Ty::Closure { .. }) {
                    FirInstructionKind::CallClosure {
                        closure: callee,
                        args,
                        tail,
                    }
                } else {
                    FirInstructionKind::CallIndirect { callee, args, tail }
                };
                self.emit_value(expr.span, typed.ty, kind)
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn lower_resolved_call(
        &mut self,
        expr: &HirExpr,
        target: DefId,
        method: bool,
        receiver: Option<ResolvedReceiver>,
        arguments: &[ResolvedCallArgument],
        result_ty: Ty,
        tail: bool,
    ) -> FirValueId {
        let HirExprKind::Call { callee, args } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/call-shape",
                "resolved call is not a call HIR node",
            );
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
        let target_params = target_body.params.clone();
        let mut placed: Vec<Option<FirValueId>> = vec![None; target_params.len()];
        let mut parameter_locals = BTreeMap::new();
        let offset = if method { 1 } else { 0 };

        if method {
            let HirExprKind::Member { base, .. } = &callee.kind else {
                self.diagnostic(
                    expr.span,
                    "fir/method-shape",
                    "resolved method call has no member receiver",
                );
                return self.poison(expr.span, result_ty);
            };
            let receiver_value = match receiver.unwrap_or(ResolvedReceiver::Value) {
                ResolvedReceiver::Value => self.lower_expr(base),
                ResolvedReceiver::SharedReference | ResolvedReceiver::MutableReference => {
                    let expected_ref = target_params
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
                                mutable: matches!(
                                    receiver,
                                    Some(ResolvedReceiver::MutableReference)
                                ),
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
            if let Some((source, ty)) = target_params.first() {
                let local = self.synthetic_local(ty.clone());
                self.emit_void(
                    base.span,
                    FirInstructionKind::Store {
                        place: FirPlace::Local { local },
                        value: receiver_value,
                    },
                );
                parameter_locals.insert(*source, local);
                placed[0] = Some(receiver_value);
            }
        }

        if arguments.len() + offset != target_params.len() {
            self.diagnostic(
                expr.span,
                "fir/call-plan",
                "resolved call plan does not contain exactly one entry per non-receiver parameter",
            );
        }

        for (slot, argument) in arguments.iter().enumerate() {
            let parameter = slot + offset;
            let Some((source_local, parameter_ty)) = target_params.get(parameter).cloned() else {
                self.diagnostic(
                    expr.span,
                    "fir/call-plan",
                    "call plan references an invalid parameter",
                );
                continue;
            };
            let value = match argument {
                ResolvedCallArgument::Explicit { argument } => {
                    let Some(arg) = args.get(*argument) else {
                        self.diagnostic(
                            expr.span,
                            "fir/call-plan",
                            "call plan references a missing explicit argument",
                        );
                        continue;
                    };
                    self.lower_expr(arg_value(arg))
                }
                ResolvedCallArgument::Default { parameter, value } => {
                    if *parameter != source_local {
                        self.diagnostic(
                            expr.span,
                            "fir/call-plan",
                            "default argument is attached to the wrong target parameter",
                        );
                    }
                    self.lower_default_argument(target, value, &parameter_locals)
                }
            };
            let local = self.synthetic_local(parameter_ty);
            self.emit_void(
                expr.span,
                FirInstructionKind::Store {
                    place: FirPlace::Local { local },
                    value,
                },
            );
            parameter_locals.insert(source_local, local);
            placed[parameter] = Some(value);
        }

        if placed.iter().any(Option::is_none) {
            self.diagnostic(
                expr.span,
                "fir/call-plan-incomplete",
                "typed HIR call plan left a target parameter without a value",
            );
            for (index, slot) in placed.iter_mut().enumerate() {
                if slot.is_none() {
                    let ty = target_params
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
            FirInstructionKind::Call { target, args, tail },
        )
    }

    fn lower_default_argument(
        &mut self,
        target: DefId,
        value: &HirExpr,
        parameter_locals: &BTreeMap<LocalId, FirLocalId>,
    ) -> FirValueId {
        let all_typed = self.all_typed;
        let Some(target_body) = all_typed.functions.get(&target) else {
            self.diagnostic(
                value.span,
                "fir/call-target-signature",
                format!("missing typed body for default argument target {target:?}"),
            );
            return self.poison(value.span, Ty::Error);
        };
        let target_exprs = target_body
            .expressions
            .iter()
            .map(|expr| (expr.id, expr))
            .collect();
        let saved_exprs = std::mem::replace(&mut self.exprs, target_exprs);
        let saved_locals = std::mem::replace(&mut self.local_map, parameter_locals.clone());
        let saved_constants = std::mem::replace(
            &mut self.local_constants,
            target_body.local_constants.clone(),
        );
        let result = self.lower_expr(value);
        self.exprs = saved_exprs;
        self.local_map = saved_locals;
        self.local_constants = saved_constants;
        result
    }

    fn lower_try(
        &mut self,
        expr: &HirExpr,
        _source_error: &Ty,
        _target_error: &Ty,
        result_ty: Ty,
    ) -> FirValueId {
        let HirExprKind::Try { value } = &expr.kind else {
            self.diagnostic(
                expr.span,
                "fir/try-shape",
                "resolved try is not a try HIR node",
            );
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
            self.active_return_type.clone(),
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

    fn sia_immediate_u8(&mut self, expr: &HirExpr, what: &str) -> u8 {
        if let Some(value) = integer_literal_u8(expr) {
            return value;
        }
        let constant = match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Def(def) => self.all_typed.constants.get(&def),
                ResolvedName::Local(local) => self.local_constants.get(&local),
                _ => None,
            },
            _ => None,
        };
        if let Some(ConstValue::Integer { value }) = constant {
            if let Ok(value) = value.to_string().parse::<u8>() {
                return value;
            }
        }
        self.diagnostic(
            expr.span,
            "fir/sia32-immediate",
            format!("{what} must be a compile-time u8 constant"),
        );
        0
    }

    fn sia_gpr_number(&mut self, expr: &HirExpr) -> u8 {
        let register = self.sia_immediate_u8(expr, "SIA32 GPR number");
        if register > 15 {
            self.diagnostic(
                expr.span,
                "fir/sia32-gpr-range",
                format!("SIA32 GPR number must be in r0..r15, got r{register}"),
            );
            return 0;
        }
        register
    }

    fn lower_name(&mut self, span: Span, name: ResolvedName, ty: Ty) -> FirValueId {
        match name {
            ResolvedName::Local(local) => {
                if let Some(value) = self.local_constants.get(&local).cloned() {
                    return self.emit_const_value(span, ty, value);
                }
                let Some(place) = self.place_for_local(local) else {
                    self.diagnostic(span, "fir/local", "unknown local in FIR lowering");
                    return self.poison(span, ty);
                };
                self.emit_value(span, ty, FirInstructionKind::Load { place })
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
                if let HirExprKind::Name { reference } = &value.kind {
                    if let ResolvedName::Def(global) = reference.root {
                        if self.all_typed.global_types.contains_key(&global) {
                            return self.emit_value(
                                span,
                                ty,
                                FirInstructionKind::AddressOfGlobal {
                                    global,
                                    mutable: matches!(op, UnaryOp::AddressOfMut),
                                },
                            );
                        }
                    }
                }
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
            self.diagnostic(
                expr.span,
                "fir/place",
                "expression is not a lowerable place",
            );
        }
        place
    }

    fn try_place(&mut self, expr: &HirExpr) -> Option<FirPlace> {
        match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Local(local) => self.place_for_local(local),
                ResolvedName::Def(global) if self.all_typed.global_types.contains_key(&global) => {
                    // Preserve a global root as the dedicated FIR address
                    // operation, then reuse the normal reference/place path
                    // for member and index projections. This keeps global
                    // symbol identity through code generation instead of
                    // pretending the global is a synthetic local.
                    let inner = self.all_typed.global_types[&global].clone();
                    let mutable = self.all_typed.mutable_globals.contains(&global);
                    let address = self.emit_value(
                        expr.span,
                        Ty::Reference {
                            mutable,
                            inner: Box::new(inner),
                        },
                        FirInstructionKind::AddressOfGlobal { global, mutable },
                    );
                    Some(FirPlace::Deref { address })
                }
                _ => None,
            },
            HirExprKind::Member { base, name } => {
                self.try_place(base).map(|base| FirPlace::Field {
                    base: Box::new(base),
                    field: name.clone(),
                })
            }
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
            } => {
                let value_ty = self.expr_ty(value);
                let address = self.lower_expr(value);
                if let Ty::Pointer { volatile, .. } = value_ty {
                    let typed = self.typed_expr(expr)?.clone();
                    let TypedExprKind::UnsafeOperation {
                        operation:
                            UnsafeOperationKind::RawDereference {
                                volatile: planned_volatile,
                            },
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
            HirPatternKind::As {
                local,
                pattern: inner,
            } => {
                self.store_local(pattern.span, *local, value);
                self.bind_irrefutable_pattern(inner, value, ty);
            }
            HirPatternKind::Struct { fields, .. } => {
                for field in fields {
                    let local_ty = field
                        .shorthand_local
                        .and_then(|id| self.typed.local_types.get(&id).cloned())
                        .or_else(|| {
                            field
                                .pattern
                                .as_ref()
                                .and_then(first_bound_local)
                                .and_then(|id| self.typed.local_types.get(&id).cloned())
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
                    Ty::Array { element, .. } | Ty::Slice { element, .. } => {
                        element.as_ref().clone()
                    }
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
            self.diagnostic(
                span,
                "fir/local",
                format!("missing local mapping for {source:?}"),
            );
        }
    }

    fn poison(&mut self, span: Span, ty: Ty) -> FirValueId {
        self.emit_value(span, ty, FirInstructionKind::Poison)
    }
}

fn first_bound_local(pattern: &HirPattern) -> Option<LocalId> {
    match &pattern.kind {
        HirPatternKind::Binding { local, .. } | HirPatternKind::As { local, .. } => Some(*local),
        HirPatternKind::Some { value }
        | HirPatternKind::Ok { value }
        | HirPatternKind::Err { value } => first_bound_local(value),
        HirPatternKind::Sequence { items, rest } => {
            items.iter().find_map(first_bound_local).or(*rest)
        }
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

fn usize_ty() -> Ty {
    Ty::Int {
        signed: false,
        width: IntWidth::Pointer,
    }
}

fn bit_mask(width: u32) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

fn integer_width_bits(ty: &Ty) -> Option<u32> {
    match ty {
        Ty::Int {
            width: IntWidth::W8,
            ..
        } => Some(8),
        Ty::Int {
            width: IntWidth::W16,
            ..
        } => Some(16),
        Ty::Int {
            width: IntWidth::W32,
            ..
        } => Some(32),
        Ty::Int {
            width: IntWidth::W64,
            ..
        } => Some(64),
        _ => None,
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
        Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } | Ty::Optional { inner } => {
            fir_type_is_concrete(inner)
        }
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
            message: format!(
                "function {:?} has return type {:?}; expected a concrete FIR type",
                function.owner, function.return_type
            ),
        });
    }
    for local in function.locals.values() {
        if !fir_type_is_concrete(&local.ty) {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-type".into(),
                message: format!(
                    "function {:?} local {:?} has type {:?}; expected a concrete FIR type",
                    function.owner, local.id, local.ty
                ),
            });
        }
    }
    for closure in function.closures.values() {
        let entry_owner = function
            .blocks
            .get(closure.entry.0 as usize)
            .and_then(|block| block.closure);
        if entry_owner != Some(closure.id) {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-closure-entry".into(),
                message: format!(
                    "function {:?} closure {:?} has entry block {:?} with closure owner {:?}; expected {:?} within {block_count} blocks",
                    function.owner,
                    closure.id,
                    closure.entry,
                    entry_owner,
                    Some(closure.id)
                ),
            });
        }
        if !fir_type_is_concrete(&closure.return_type)
            || closure
                .captures
                .iter()
                .any(|capture| !fir_type_is_concrete(&capture.ty))
        {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-closure-type".into(),
                message: format!(
                    "function {:?} closure {:?} has return type {:?} and capture types {:?}; expected concrete FIR types",
                    function.owner,
                    closure.id,
                    closure.return_type,
                    closure
                        .captures
                        .iter()
                        .map(|capture| (&capture.local, &capture.ty))
                        .collect::<Vec<_>>()
                ),
            });
        }
    }

    for block in &function.blocks {
        if block.terminator.is_none() {
            diagnostics.push(FirDiagnostic {
                span: Span::new(0, 0),
                code: "fir/verify-terminator".into(),
                message: format!(
                    "function {:?} block {:?} has no terminator",
                    function.owner, block.id
                ),
            });
        }
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if let Some(result) = instruction.result {
                if !definitions.insert(result) {
                    diagnostics.push(FirDiagnostic {
                        span: instruction.span,
                        code: "fir/verify-value".into(),
                        message: format!(
                            "function {:?} block {:?} instruction {instruction_index} defines value {result:?} more than once",
                            function.owner, block.id
                        ),
                    });
                }
                if !function.value_types.contains_key(&result) {
                    diagnostics.push(FirDiagnostic {
                        span: instruction.span,
                        code: "fir/verify-value".into(),
                        message: format!(
                            "function {:?} block {:?} instruction {instruction_index} defines value {result:?} with no type",
                            function.owner, block.id
                        ),
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
                FirTerminator::Select { cases, .. } => cases
                    .iter()
                    .map(|case| match case {
                        FirSelectCase::Receive { target, .. }
                        | FirSelectCase::Timeout { target, .. } => *target,
                    })
                    .collect(),
                FirTerminator::Return { .. } | FirTerminator::Unreachable => Vec::new(),
            };
            for target in targets {
                if target.0 >= block_count {
                    diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-target".into(),
                        message: format!(
                            "function {:?} block {:?} terminator targets missing block {target:?}; valid block indexes are 0..{block_count}",
                            function.owner, block.id
                        ),
                    });
                }
            }
            if let FirTerminator::Branch { condition, .. } = term {
                if function.value_types.get(condition) != Some(&Ty::Bool) {
                    diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-branch".into(),
                        message: format!(
                            "function {:?} block {:?} branch condition {condition:?} has type {:?}; expected Bool",
                            function.owner,
                            block.id,
                            function.value_types.get(condition)
                        ),
                    });
                }
            }
            if let FirTerminator::Select { operation, cases } = term {
                if *operation != RuntimeOperationId::SelectWait || cases.is_empty() {
                    diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-select".into(),
                        message: format!(
                            "function {:?} block {:?} select terminator has operation {:?} and {} cases; expected SelectWait and at least one case",
                            function.owner,
                            block.id,
                            operation,
                            cases.len()
                        ),
                    });
                }
                for (case_index, case) in cases.iter().enumerate() {
                    if let FirSelectCase::Receive {
                        operation,
                        payload,
                        payload_type,
                        ..
                    } = case
                    {
                        if *operation != RuntimeOperationId::ChannelReceive
                            || function.locals.get(payload).map(|local| &local.ty)
                                != Some(payload_type)
                        {
                            diagnostics.push(FirDiagnostic {
                                span: Span::new(0, 0),
                                code: "fir/verify-select".into(),
                                message: format!(
                                    "function {:?} block {:?} select case {case_index} has operation {:?} and payload local {:?} type {:?}; expected ChannelReceive and {:?}",
                                    function.owner,
                                    block.id,
                                    operation,
                                    payload,
                                    function.locals.get(payload).map(|local| &local.ty),
                                    payload_type
                                ),
                            });
                        }
                    } else if let FirSelectCase::Timeout { operation, .. } = case {
                        if *operation != RuntimeOperationId::SelectTimeout {
                            diagnostics.push(FirDiagnostic {
                                span: Span::new(0, 0),
                                code: "fir/verify-select".into(),
                                message: format!(
                                    "function {:?} block {:?} select case {case_index} has timeout operation {:?}; expected SelectTimeout",
                                    function.owner, block.id, operation
                                ),
                            });
                        }
                    }
                }
            }
            if let FirTerminator::Return { value } = term {
                let expected = block
                    .closure
                    .and_then(|id| function.closures.get(&id))
                    .map(|closure| &closure.return_type)
                    .unwrap_or(&function.return_type);
                let actual = value
                    .as_ref()
                    .and_then(|value| function.value_types.get(value));
                match (value, expected) {
                    (None, Ty::Void) => {}
                    (Some(value), expected)
                        if function.value_types.get(value) == Some(expected) => {}
                    _ => diagnostics.push(FirDiagnostic {
                        span: Span::new(0, 0),
                        code: "fir/verify-return".into(),
                        message: format!(
                            "function {:?} block {:?} return value {value:?} has type {actual:?}; expected {expected:?}",
                            function.owner, block.id
                        ),
                    }),
                }
            }
        }
    }
    diagnostics
}

fn integer_literal_u8(expr: &HirExpr) -> Option<u8> {
    match &expr.kind {
        HirExprKind::Integer { text } => {
            let raw = text.split(['u', 'i']).next().unwrap_or(text);
            if let Some(hex) = raw.strip_prefix("0x") {
                u8::from_str_radix(hex, 16).ok()
            } else {
                raw.parse().ok()
            }
        }
        _ => None,
    }
}
