from pathlib import Path

# Type checker integration corrections after the main phase-2 transformation.
p = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = p.read_text()
text = text.replace('#[serde(tag = "pattern", rename_all = "snake_case")]', '#[serde(tag = "pattern_kind", rename_all = "snake_case")]', 1)

old = '''pub struct TypedBody {
    pub owner: DefId,
    pub params: Vec<(LocalId, Ty)>,
    pub return_type: Ty,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
    pub unsafe_expressions: BTreeSet<ExprId>,
}
'''
new = '''pub struct TypedBody {
    pub owner: DefId,
    pub params: Vec<(LocalId, Ty)>,
    pub return_type: Ty,
    pub local_types: BTreeMap<LocalId, Ty>,
    pub local_constants: BTreeMap<LocalId, ConstValue>,
    pub expressions: Vec<TypedExpr>,
    pub unsafe_expressions: BTreeSet<ExprId>,
    pub call_plans: BTreeMap<ExprId, ResolvedCallPlan>,
    pub closure_plans: BTreeMap<ExprId, TypedClosurePlan>,
    pub match_plans: BTreeMap<ExprId, TypedMatchPlan>,
    pub select_receives: BTreeMap<ExprId, TypedSelectReceive>,
}
'''
if old not in text:
    raise SystemExit("TypedBody base shape not found")
text = text.replace(old, new, 1)

old = '''                expressions: checker.expressions,
                unsafe_expressions: checker.unsafe_expressions,
            },
        );
'''
# There are two such literals; the first function body already gained plan fields.
# Target the global initializer by finding it after `TypedGlobal {`.
pos = text.find("TypedGlobal {")
if pos < 0:
    raise SystemExit("TypedGlobal initializer marker missing")
sub = text[pos:]
if old not in sub:
    raise SystemExit("TypedGlobal tail not found")
sub = sub.replace(
    old,
    '''                expressions: checker.expressions,
                unsafe_expressions: checker.unsafe_expressions,
                call_plans: checker.call_plans,
                closure_plans: checker.closure_plans,
                match_plans: checker.match_plans,
                select_receives: checker.select_receives,
            },
        );
''',
    1,
)
text = text[:pos] + sub
p.write_text(text)

# Parser's recursive validation treats a resolved context access as a leaf.
p = Path("crates/forge-frontend/src/parser_v1.rs")
text = p.read_text()
old = '''        | ExprKind::None
        | ExprKind::Keyword { .. }
        | ExprKind::Path { .. }
'''
new = '''        | ExprKind::None
        | ExprKind::Keyword { .. }
        | ExprKind::Context { .. }
        | ExprKind::Path { .. }
'''
if old not in text:
    raise SystemExit("parser validation leaf block missing")
text = text.replace(old, new, 1)
p.write_text(text)

# Public semantic plans are part of the frontend/FIR boundary and should be
# available to tests/backends without reaching into a private module path.
p = Path("crates/forge-frontend/src/lib.rs")
text = p.read_text()
old = '''pub use typecheck::{
    type_check_module, ConstValue, IntWidth, ResolvedReceiver, Ty, TypeCheckOutput, TypeDiagnostic,
    TypedBody, TypedExpr, TypedExprKind,
};
'''
new = '''pub use typecheck::{
    type_check_module, BitStructFieldInfo, BitStructInfo, CaptureMode, ConstValue, ContextSlot,
    IntWidth, ResolvedCallArgument, ResolvedCallPlan, ResolvedReceiver, Ty, TypeCheckOutput,
    TypeDiagnostic, TypedBody, TypedCapture, TypedClosurePlan, TypedExpr, TypedExprKind,
    TypedGlobal, TypedMatchPlan, TypedPattern, TypedPatternField, TypedPatternKind,
    TypedSelectReceive,
};
'''
if old not in text:
    raise SystemExit("typecheck reexport block missing")
text = text.replace(old, new, 1)
p.write_text(text)

print("semantic normalization integration postpatch applied")
