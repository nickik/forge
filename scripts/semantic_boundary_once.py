from pathlib import Path
import re


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if old not in text:
        raise SystemExit(f"missing exact block: {label}")
    return text.replace(old, new, 1)


def regex_once(text: str, pattern: str, repl, label: str) -> str:
    out, count = re.subn(pattern, repl, text, count=1, flags=re.S)
    if count != 1:
        raise SystemExit(f"regex {label}: expected 1 match, got {count}")
    return out

# ---------------------------------------------------------------------------
# HIR: preserve the global binding class. Runtime-init lowering needs to know
# const/val/var instead of reconstructing it from the AST later.
# ---------------------------------------------------------------------------
body = Path("crates/forge-frontend/src/body_hir_v1.rs")
text = body.read_text()
text = replace_once(
    text,
    """pub struct HirGlobalBody {\n    pub owner: DefId,\n    pub ty: Option<HirType>,\n    pub value: HirExpr,\n}\n""",
    """pub struct HirGlobalBody {\n    pub owner: DefId,\n    pub binding: ast::BindingKind,\n    pub ty: Option<HirType>,\n    pub value: HirExpr,\n}\n""",
    "HirGlobalBody binding",
)
text = replace_once(
    text,
    """                    HirGlobalBody {\n                        owner,\n                        ty,\n                        value: expr,\n                    },\n""",
    """                    HirGlobalBody {\n                        owner,\n                        binding: value.binding,\n                        ty,\n                        value: expr,\n                    },\n""",
    "global lowering binding",
)
body.write_text(text)

# ---------------------------------------------------------------------------
# Type checking: semantic bitstruct metadata, explicit unsafe provenance,
# typed global initializer bodies, and map-pattern rejection at the semantic
# layer rather than letting Unknown escape into FIR.
# ---------------------------------------------------------------------------
typecheck = Path("crates/forge-frontend/src/typecheck_v1.rs")
text = typecheck.read_text()

insert_after_const = """pub enum ConstValue {\n    Integer { value: i128 },\n    Bool { value: bool },\n    Char { value: char },\n}\n"""
semantic_types = r'''

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BitStructFieldInfo {
    pub offset: u32,
    pub width: u32,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BitStructInfo {
    pub storage: Ty,
    pub storage_bits: u32,
    pub fields: BTreeMap<String, BitStructFieldInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedGlobal {
    pub owner: DefId,
    pub binding: ast::BindingKind,
    pub ty: Ty,
    pub initializer: HirExpr,
    pub expressions: Vec<TypedExpr>,
    pub unsafe_expressions: BTreeSet<ExprId>,
}
'''
text = replace_once(text, insert_after_const, insert_after_const + semantic_types, "semantic public structs")

text = replace_once(
    text,
    """pub struct TypedBody {\n    pub owner: DefId,\n    pub params: Vec<(LocalId, Ty)>,\n    pub return_type: Ty,\n    pub local_types: BTreeMap<LocalId, Ty>,\n    pub local_constants: BTreeMap<LocalId, ConstValue>,\n    pub expressions: Vec<TypedExpr>,\n}\n""",
    """pub struct TypedBody {\n    pub owner: DefId,\n    pub params: Vec<(LocalId, Ty)>,\n    pub return_type: Ty,\n    pub local_types: BTreeMap<LocalId, Ty>,\n    pub local_constants: BTreeMap<LocalId, ConstValue>,\n    pub expressions: Vec<TypedExpr>,\n    pub unsafe_expressions: BTreeSet<ExprId>,\n}\n""",
    "TypedBody unsafe provenance",
)
text = replace_once(
    text,
    """pub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub global_types: BTreeMap<DefId, Ty>,\n    pub constants: BTreeMap<DefId, ConstValue>,\n    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,\n    pub metadata: MetadataTable,\n    pub diagnostics: Vec<TypeDiagnostic>,\n}\n""",
    """pub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub globals: BTreeMap<DefId, TypedGlobal>,\n    pub global_types: BTreeMap<DefId, Ty>,\n    pub bitstructs: BTreeMap<DefId, BitStructInfo>,\n    pub constants: BTreeMap<DefId, ConstValue>,\n    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,\n    pub metadata: MetadataTable,\n    pub diagnostics: Vec<TypeDiagnostic>,\n}\n""",
    "TypeCheckOutput semantic tables",
)
text = replace_once(
    text,
    """enum TypeInfoKind {\n    Distinct(Ty),\n    Alias(Ty),\n    Struct(BTreeMap<String, FieldInfo>),\n    Enum(BTreeSet<String>),\n    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),\n    Nominal,\n}\n""",
    """enum TypeInfoKind {\n    Distinct(Ty),\n    Alias(Ty),\n    Struct(BTreeMap<String, FieldInfo>),\n    Enum(BTreeSet<String>),\n    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),\n    BitStruct(BitStructInfo),\n    Nominal,\n}\n""",
    "TypeInfoKind bitstruct",
)

# Insert bitstruct layout validation immediately before the main type-check entry.
marker = "pub fn type_check_module(\n"
if marker not in text:
    raise SystemExit("type_check_module marker missing")
bitstruct_helpers = r'''
fn collect_bitstruct_info(
    source: &ast::SourceFile,
    diagnostics: &mut Vec<TypeDiagnostic>,
) -> BTreeMap<DefId, BitStructInfo> {
    let mut result = BTreeMap::new();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let DeclKind::BitStruct(bitstruct) = &declaration.kind.kind else {
            continue;
        };
        let owner = DefId(index as u32);
        let (storage, storage_bits) = match &bitstruct.storage.kind {
            ast::TypeKind::Named { path } if path.segments.len() == 1 => match path.segments[0].as_str() {
                "u8" => (Ty::Int { signed: false, width: IntWidth::W8 }, 8),
                "u16" => (Ty::Int { signed: false, width: IntWidth::W16 }, 16),
                "u32" => (Ty::Int { signed: false, width: IntWidth::W32 }, 32),
                "u64" => (Ty::Int { signed: false, width: IntWidth::W64 }, 64),
                _ => {
                    diagnostics.push(TypeDiagnostic {
                        span: bitstruct.storage.span,
                        code: "bitstruct/storage".into(),
                        message: "bitstruct storage must be exactly u8, u16, u32, or u64".into(),
                    });
                    continue;
                }
            },
            _ => {
                diagnostics.push(TypeDiagnostic {
                    span: bitstruct.storage.span,
                    code: "bitstruct/storage".into(),
                    message: "bitstruct storage must be exactly u8, u16, u32, or u64".into(),
                });
                continue;
            }
        };
        let mut offset = 0u32;
        let mut fields = BTreeMap::new();
        for field in &bitstruct.fields {
            if field.width == 0 || field.width > storage_bits {
                diagnostics.push(TypeDiagnostic {
                    span: declaration.span,
                    code: "bitstruct/field-width".into(),
                    message: format!(
                        "bitstruct field `{}` has invalid width {}; width must be in 1..={storage_bits}",
                        field.name, field.width
                    ),
                });
                continue;
            }
            let ty = match field.width {
                1 => Ty::Bool,
                2..=8 => Ty::Int { signed: false, width: IntWidth::W8 },
                9..=16 => Ty::Int { signed: false, width: IntWidth::W16 },
                17..=32 => Ty::Int { signed: false, width: IntWidth::W32 },
                _ => Ty::Int { signed: false, width: IntWidth::W64 },
            };
            fields.insert(
                field.name.clone(),
                BitStructFieldInfo {
                    offset,
                    width: field.width,
                    ty,
                },
            );
            offset = offset.saturating_add(field.width);
        }
        if offset != storage_bits {
            diagnostics.push(TypeDiagnostic {
                span: declaration.span,
                code: "bitstruct/size".into(),
                message: format!(
                    "bitstruct fields total {offset} bits but storage requires exactly {storage_bits}; spell unused bits as reserved fields"
                ),
            });
        }
        result.insert(
            owner,
            BitStructInfo {
                storage,
                storage_bits,
                fields,
            },
        );
    }
    result
}

'''
text = text.replace(marker, bitstruct_helpers + marker, 1)

text = replace_once(
    text,
    """    validate_declaration_array_lengths(source, module, &constant_values, &mut output.diagnostics);\n    let env = ModuleTypeEnv::build(source, module, &constant_values);\n""",
    """    validate_declaration_array_lengths(source, module, &constant_values, &mut output.diagnostics);\n    let bitstructs = collect_bitstruct_info(source, &mut output.diagnostics);\n    output.bitstructs = bitstructs.clone();\n    let env = ModuleTypeEnv::build(source, module, &constant_values, &bitstructs);\n""",
    "collect bitstructs",
)
text = replace_once(
    text,
    """    fn build(\n        source: &ast::SourceFile,\n        module: &HirModule,\n        constants: &BTreeMap<DefId, ConstValue>,\n    ) -> Self {\n""",
    """    fn build(\n        source: &ast::SourceFile,\n        module: &HirModule,\n        constants: &BTreeMap<DefId, ConstValue>,\n        bitstructs: &BTreeMap<DefId, BitStructInfo>,\n    ) -> Self {\n""",
    "ModuleTypeEnv build signature",
)

# Add bitstruct structural info next to structs/enums/tagged.
text = replace_once(
    text,
    """                DeclKind::Tagged(value) => {\n                    let variants = value\n                        .variants\n                        .iter()\n                        .map(|variant| {\n                            let fields = variant\n                                .fields\n                                .iter()\n                                .map(|field| {\n                                    (\n                                        field.name.clone(),\n                                        FieldInfo {\n                                            ty: env.lower_ast_type(&field.ty, module),\n                                            has_default: field.default.is_some(),\n                                        },\n                                    )\n                                })\n                                .collect();\n                            (variant.name.clone(), fields)\n                        })\n                        .collect();\n                    env.types.insert(\n                        id,\n                        TypeInfo {\n                            kind: TypeInfoKind::Tagged(variants),\n                        },\n                    );\n                }\n                _ => {}\n""",
    """                DeclKind::Tagged(value) => {\n                    let variants = value\n                        .variants\n                        .iter()\n                        .map(|variant| {\n                            let fields = variant\n                                .fields\n                                .iter()\n                                .map(|field| {\n                                    (\n                                        field.name.clone(),\n                                        FieldInfo {\n                                            ty: env.lower_ast_type(&field.ty, module),\n                                            has_default: field.default.is_some(),\n                                        },\n                                    )\n                                })\n                                .collect();\n                            (variant.name.clone(), fields)\n                        })\n                        .collect();\n                    env.types.insert(\n                        id,\n                        TypeInfo {\n                            kind: TypeInfoKind::Tagged(variants),\n                        },\n                    );\n                }\n                DeclKind::BitStruct(_) => {\n                    if let Some(info) = bitstructs.get(&id) {\n                        env.types.insert(\n                            id,\n                            TypeInfo {\n                                kind: TypeInfoKind::BitStruct(info.clone()),\n                            },\n                        );\n                    }\n                }\n                _ => {}\n""",
    "bitstruct structural info",
)

# Resolve bitstruct nominal identity like structs/enums/tagged.
text = text.replace(
    """            Some(TypeInfoKind::Distinct(_))\n            | Some(TypeInfoKind::Struct(_))\n            | Some(TypeInfoKind::Enum(_))\n            | Some(TypeInfoKind::Tagged(_))\n            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),\n""",
    """            Some(TypeInfoKind::Distinct(_))\n            | Some(TypeInfoKind::Struct(_))\n            | Some(TypeInfoKind::Enum(_))\n            | Some(TypeInfoKind::Tagged(_))\n            | Some(TypeInfoKind::BitStruct(_))\n            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),\n""",
    1,
)
if "Some(TypeInfoKind::BitStruct(_))" not in text:
    raise SystemExit("failed to add BitStruct to resolve_type_def")

# Member lookup returns the ordinary Forge field type.
text = replace_once(
    text,
    """                Some(TypeInfoKind::Struct(fields)) => fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(_) => MemberLookup::Unsupported,\n""",
    """                Some(TypeInfoKind::Struct(fields)) => fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(TypeInfoKind::BitStruct(info)) => info\n                    .fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(_) => MemberLookup::Unsupported,\n""",
    "bitstruct member lookup",
)

# Typed function bodies retain authorization of each unsafe operation.
text = replace_once(
    text,
    """                local_constants: checker.local_constants,\n                expressions: checker.expressions,\n""",
    """                local_constants: checker.local_constants,\n                expressions: checker.expressions,\n                unsafe_expressions: checker.unsafe_expressions,\n""",
    "function unsafe expressions",
)

# Globals keep their typed expression table so runtime initialization can lower
# exactly like a small body instead of reparsing/retyping the AST.
text = replace_once(
    text,
    """        output.global_types.insert(*owner, ty);\n    }\n\n    output\n}\n""",
    """        output.global_types.insert(*owner, ty.clone());\n        output.globals.insert(\n            *owner,\n            TypedGlobal {\n                owner: *owner,\n                binding: global.binding,\n                ty,\n                initializer: global.value.clone(),\n                expressions: checker.expressions,\n                unsafe_expressions: checker.unsafe_expressions,\n            },\n        );\n    }\n\n    output\n}\n""",
    "typed global initializer",
)

# BodyChecker state for THIR-style unsafe provenance.
text = replace_once(
    text,
    """    mutable_locals: BTreeSet<LocalId>,\n    expressions: Vec<TypedExpr>,\n    diagnostics: &'d mut Vec<TypeDiagnostic>,\n""",
    """    mutable_locals: BTreeSet<LocalId>,\n    expressions: Vec<TypedExpr>,\n    unsafe_depth: u32,\n    unsafe_expressions: BTreeSet<ExprId>,\n    diagnostics: &'d mut Vec<TypeDiagnostic>,\n""",
    "BodyChecker unsafe fields",
)
text = replace_once(
    text,
    """            mutable_locals: BTreeSet::new(),\n            expressions: Vec::new(),\n            diagnostics,\n""",
    """            mutable_locals: BTreeSet::new(),\n            expressions: Vec::new(),\n            unsafe_depth: 0,\n            unsafe_expressions: BTreeSet::new(),\n            diagnostics,\n""",
    "BodyChecker unsafe init",
)

# Unsafe blocks are semantic authorization scopes; ordinary blocks remain safe.
text = replace_once(
    text,
    """            HirStmtKind::DeferBlock { block }\n            | HirStmtKind::Unsafe { block }\n            | HirStmtKind::Block { block } => self.check_block(block),\n""",
    """            HirStmtKind::DeferBlock { block } | HirStmtKind::Block { block } => {\n                self.check_block(block)\n            }\n            HirStmtKind::Unsafe { block } => {\n                self.unsafe_depth += 1;\n                self.check_block(block);\n                self.unsafe_depth -= 1;\n            }\n""",
    "unsafe block scope",
)

# Raw pointer dereference requires an active unsafe scope and records provenance.
text = replace_once(
    text,
    """            HirExprKind::Unary { op, value } => {\n                let v = self.check_expr(value, expected);\n                self.check_unary(expr.span, *op, v)\n            }\n""",
    """            HirExprKind::Unary { op, value } => {\n                let v = self.check_expr(value, expected);\n                if matches!(op, UnaryOp::Deref) && matches!(&v, Ty::Pointer { .. }) {\n                    if self.unsafe_depth == 0 {\n                        self.diagnostic(\n                            expr.span,\n                            \"unsafe/required\",\n                            \"raw pointer dereference requires an `unsafe` block\",\n                        );\n                    } else {\n                        self.unsafe_expressions.insert(expr.id);\n                    }\n                }\n                self.check_unary(expr.span, *op, v)\n            }\n""",
    "raw pointer deref unsafe",
)

# Type calls need ExprId to retain unsafe conversion authorization.
text = replace_once(
    text,
    """            HirExprKind::TypeCall { target, args } => self.check_type_call(expr.span, target, args),\n""",
    """            HirExprKind::TypeCall { target, args } => {\n                self.check_type_call(expr.id, expr.span, target, args)\n            }\n""",
    "type call ExprId",
)
text = replace_once(
    text,
    """    fn check_type_call(&mut self, span: Span, target: &HirTypeRef, args: &[HirCallArg]) -> Ty {\n""",
    """    fn check_type_call(\n        &mut self,\n        expr_id: ExprId,\n        span: Span,\n        target: &HirTypeRef,\n        args: &[HirCallArg],\n    ) -> Ty {\n""",
    "check_type_call signature",
)
text = replace_once(
    text,
    """        let source = self.check_expr(arg_value(&args[0]), None);\n        match &target_ty {\n""",
    """        let source = self.check_expr(arg_value(&args[0]), None);\n        let raw_conversion = matches!(\n            (&target_ty, &source),\n            (Ty::Pointer { .. }, Ty::Int { .. } | Ty::Byte)\n                | (Ty::Int { .. } | Ty::Byte, Ty::Pointer { .. })\n                | (Ty::Pointer { .. }, Ty::Pointer { .. })\n        );\n        if raw_conversion {\n            if self.unsafe_depth == 0 {\n                self.diagnostic(\n                    span,\n                    \"unsafe/required\",\n                    \"raw pointer conversion requires an `unsafe` block\",\n                );\n            } else {\n                self.unsafe_expressions.insert(expr_id);\n            }\n        }\n        match &target_ty {\n""",
    "raw conversion unsafe",
)
text = replace_once(
    text,
    """        if is_numeric_concrete(target) && is_numeric_concrete(source) {\n            return true;\n        }\n""",
    """        if is_numeric_concrete(target) && is_numeric_concrete(source) {\n            return true;\n        }\n        if matches!(\n            (target, source),\n            (Ty::Pointer { .. }, Ty::Int { .. } | Ty::Byte)\n                | (Ty::Int { .. } | Ty::Byte, Ty::Pointer { .. })\n                | (Ty::Pointer { .. }, Ty::Pointer { .. })\n        ) {\n            return true;\n        }\n""",
    "pointer explicit conversion",
)

# Reject map patterns at the semantic layer until Forge has a real typed
# collection-pattern protocol. Rust similarly avoids pretending arbitrary map
# lookup is a structural pattern primitive.
m = re.search(r"\n    fn check_pattern\([\s\S]*?\n    \) \{\n", text)
if not m:
    raise SystemExit("check_pattern signature not found")
insert = m.end()
map_guard = '''        if matches!(pattern.kind, HirPatternKind::Map { .. }) {\n            self.diagnostic(\n                pattern.span,\n                "pattern/map-deferred",\n                "map/collection patterns are reserved but not part of Forge v1 until a typed collection-pattern protocol is defined",\n            );\n            return;\n        }\n'''
text = text[:insert] + map_guard + text[insert:]

typecheck.write_text(text)

# ---------------------------------------------------------------------------
# Tests for this semantic foundation slice.
# ---------------------------------------------------------------------------
tests = Path("crates/forge-frontend/tests/typecheck.rs")
t = tests.read_text()
if "bitstruct_layout_is_semantic_and_lsb_first" not in t:
    t += r'''

#[test]
fn bitstruct_layout_is_semantic_and_lsb_first() {
    let output = check(
        r#"
        module test.bitstruct_layout;
        bitstruct Status: u16 {
            ready: 1;
            error: 1;
            mode: 3;
            code: 5;
            reserved: 6;
        }
        fn mode(value: Status) -> u8 { return value.mode; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let info = output.bitstructs.values().next().expect("bitstruct info");
    assert_eq!(info.storage_bits, 16);
    assert_eq!(info.fields["ready"].offset, 0);
    assert_eq!(info.fields["ready"].ty, Ty::Bool);
    assert_eq!(info.fields["mode"].offset, 2);
    assert_eq!(info.fields["mode"].width, 3);
    assert_eq!(
        info.fields["mode"].ty,
        Ty::Int { signed: false, width: IntWidth::W8 }
    );
    assert_eq!(info.fields["reserved"].offset, 10);
}

#[test]
fn bitstruct_rejects_bad_storage_and_incomplete_layout() {
    let storage = check(
        r#"
        module test.bitstruct_bad_storage;
        bitstruct Bad: i16 { all: 16; }
        "#,
    );
    assert!(has(&storage, "bitstruct/storage"), "{:?}", storage.diagnostics);

    let size = check(
        r#"
        module test.bitstruct_bad_size;
        bitstruct Bad: u16 { low: 8; }
        "#,
    );
    assert!(has(&size, "bitstruct/size"), "{:?}", size.diagnostics);
}

#[test]
fn raw_pointer_deref_requires_unsafe_and_records_authorization() {
    let bad = check(
        r#"
        module test.raw_deref_bad;
        fn read(p: *u32) -> u32 { return *p; }
        "#,
    );
    assert!(has(&bad, "unsafe/required"), "{:?}", bad.diagnostics);

    let good = check(
        r#"
        module test.raw_deref_good;
        fn read(p: *u32) -> u32 { unsafe { return *p; } }
        "#,
    );
    assert!(good.diagnostics.is_empty(), "{:?}", good.diagnostics);
    let body = good.functions.values().next().unwrap();
    assert_eq!(body.unsafe_expressions.len(), 1);
}

#[test]
fn map_patterns_stop_at_semantic_boundary() {
    let output = check(
        r#"
        module test.map_pattern_deferred;
        fn read(value: u32) -> i32 {
            return match (value) { { :name name, .. } => 1, _ => 0 };
        }
        "#,
    );
    assert!(has(&output, "pattern/map-deferred"), "{:?}", output.diagnostics);
}

#[test]
fn runtime_global_initializer_keeps_typed_body() {
    let output = check(
        r#"
        module test.runtime_global;
        fn seed() -> u32 { return 7u32; }
        val runtime_value: u32 = seed();
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let global = output
        .globals
        .values()
        .find(|global| !matches!(global.binding, forge_frontend::ast::BindingKind::Const))
        .expect("runtime global");
    assert_eq!(global.ty, Ty::Int { signed: false, width: IntWidth::W32 });
    assert!(!global.expressions.is_empty());
}
'''
    tests.write_text(t)

print("semantic boundary foundation migration applied")
