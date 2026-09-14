from pathlib import Path


def replace_once(text: str, old: str, new: str) -> str:
    if old not in text:
        raise SystemExit(f"missing replacement anchor:\n{old[:240]}")
    return text.replace(old, new, 1)


p = Path("crates/forge-frontend/src/typecheck_v1.rs")
s = p.read_text()

s = replace_once(
    s,
    """#[derive(Debug, Clone)]
struct TypeInfo {
    kind: TypeInfoKind,
}

#[derive(Debug, Clone)]
enum TypeInfoKind {
    Distinct(Ty),
    Alias(Ty),
    Nominal,
}
""",
    """#[derive(Debug, Clone)]
struct TypeInfo {
    kind: TypeInfoKind,
}

#[derive(Debug, Clone)]
struct FieldInfo {
    ty: Ty,
    has_default: bool,
}

#[derive(Debug, Clone)]
enum TypeInfoKind {
    Distinct(Ty),
    Alias(Ty),
    Struct(BTreeMap<String, FieldInfo>),
    Enum(BTreeSet<String>),
    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),
    Nominal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MemberLookup {
    Field(Ty),
    MissingField,
    Unsupported,
    Unknown,
}
""",
)

anchor = """        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            if let DeclKind::Function(function) = &declaration.kind.kind {
"""
structural_pass = """        // Once aliases/distinct types are known, record structural metadata used by
        // member access, construction and pattern typing.
        for (index, declaration) in source.declarations.iter().enumerate() {
            let id = DefId(index as u32);
            match &declaration.kind.kind {
                DeclKind::Struct(value) => {
                    let fields = value
                        .fields
                        .iter()
                        .map(|field| {
                            (
                                field.name.clone(),
                                FieldInfo {
                                    ty: env.lower_ast_type(&field.ty, module),
                                    has_default: field.default.is_some(),
                                },
                            )
                        })
                        .collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Struct(fields),
                        },
                    );
                }
                DeclKind::Enum(value) => {
                    let variants = value.variants.iter().map(|variant| variant.name.clone()).collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Enum(variants),
                        },
                    );
                }
                DeclKind::Tagged(value) => {
                    let variants = value
                        .variants
                        .iter()
                        .map(|variant| {
                            let fields = variant
                                .fields
                                .iter()
                                .map(|field| {
                                    (
                                        field.name.clone(),
                                        FieldInfo {
                                            ty: env.lower_ast_type(&field.ty, module),
                                            has_default: field.default.is_some(),
                                        },
                                    )
                                })
                                .collect();
                            (variant.name.clone(), fields)
                        })
                        .collect();
                    env.types.insert(
                        id,
                        TypeInfo {
                            kind: TypeInfoKind::Tagged(variants),
                        },
                    );
                }
                _ => {}
            }
        }

""" + anchor
s = replace_once(s, anchor, structural_pass)

s = replace_once(
    s,
    """    fn resolve_type_def(&self, id: DefId) -> Ty {
        match self.types.get(&id).map(|i| &i.kind) {
            Some(TypeInfoKind::Alias(ty)) => ty.clone(),
            Some(TypeInfoKind::Distinct(_)) | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),
            None => Ty::Unknown,
        }
    }

    fn distinct_underlying(&self, id: DefId) -> Option<&Ty> {
""",
    """    fn resolve_type_def(&self, id: DefId) -> Ty {
        match self.types.get(&id).map(|i| &i.kind) {
            Some(TypeInfoKind::Alias(ty)) => ty.clone(),
            Some(TypeInfoKind::Distinct(_))
            | Some(TypeInfoKind::Struct(_))
            | Some(TypeInfoKind::Enum(_))
            | Some(TypeInfoKind::Tagged(_))
            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),
            None => Ty::Unknown,
        }
    }

    fn lookup_member(&self, ty: &Ty, name: &str) -> MemberLookup {
        match ty {
            Ty::Reference { inner, .. } => self.lookup_member(inner, name),
            Ty::Nominal(id) => match self.types.get(id).map(|info| &info.kind) {
                Some(TypeInfoKind::Struct(fields)) => fields
                    .get(name)
                    .map(|field| MemberLookup::Field(field.ty.clone()))
                    .unwrap_or(MemberLookup::MissingField),
                Some(_) => MemberLookup::Unsupported,
                None => MemberLookup::Unknown,
            },
            Ty::Array { .. } | Ty::Slice { .. } | Ty::Str if name == "len" => {
                MemberLookup::Field(Ty::Int {
                    signed: false,
                    width: IntWidth::Pointer,
                })
            }
            Ty::Unknown | Ty::Error => MemberLookup::Unknown,
            _ => MemberLookup::Unsupported,
        }
    }

    fn distinct_underlying(&self, id: DefId) -> Option<&Ty> {
""",
)

s = replace_once(
    s,
    """            HirExprKind::Name { reference } => self.type_of_name(reference.root),
            HirExprKind::Qualified { namespace, .. } => self.env.ty_from_ref(namespace),
""",
    """            HirExprKind::Name { reference } => self.type_of_name(reference.root),
            HirExprKind::Qualified { namespace, name } => {
                let ty = self.env.ty_from_ref(namespace);
                self.check_qualified_variant(expr.span, &ty, name);
                ty
            }
""",
)

s = replace_once(
    s,
    """            HirExprKind::StructInit { namespace, .. } => self.env.ty_from_ref(namespace),
""",
    """            HirExprKind::StructInit {
                namespace,
                variant,
                fields,
            } => {
                let ty = self.env.ty_from_ref(namespace);
                self.check_struct_init(expr.span, &ty, variant.as_deref(), fields);
                ty
            }
""",
)

s = replace_once(
    s,
    """            HirExprKind::Member { base, .. } => {
                self.check_expr(base, None);
                Ty::Unknown
            }
""",
    """            HirExprKind::Member { base, name } => {
                let base_ty = self.check_expr(base, None);
                self.check_member(expr.span, &base_ty, name)
            }
""",
)

# Upgrade all existing pattern-binding entry points to semantic pattern checking.
s = s.replace("self.bind_pattern_type(pattern, &final_ty);", "self.check_pattern(pattern, &final_ty);")
s = s.replace("self.bind_pattern_type(pattern, &element);", "self.check_pattern(pattern, &element);")
s = s.replace("self.bind_pattern_type(pattern, &Ty::Unknown);", "self.check_pattern(pattern, &Ty::Unknown);")
s = s.replace("self.bind_pattern_type(&arm.pattern, &matched);", "self.check_pattern(&arm.pattern, &matched);")

s = replace_once(
    s,
    """            HirExprKind::Member { base, .. } | HirExprKind::Index { base, .. } => {
                self.check_assignment_target(base);
            }
""",
    """            HirExprKind::Member { base, .. } => {
                match self.place_type(base) {
                    Ty::Reference { mutable: true, .. } => {}
                    Ty::Reference { mutable: false, .. } => self.diagnostic(
                        target.span,
                        "assignment/immutable",
                        "cannot assign through an immutable reference",
                    ),
                    _ => self.check_assignment_target(base),
                }
            }
            HirExprKind::Index { base, .. } => {
                match self.place_type(base) {
                    Ty::Slice { mutable: true, .. } => {}
                    Ty::Slice { mutable: false, .. } => self.diagnostic(
                        target.span,
                        "assignment/immutable",
                        "cannot assign through a read-only slice",
                    ),
                    Ty::Reference { mutable: true, .. } => {}
                    Ty::Reference { mutable: false, .. } => self.diagnostic(
                        target.span,
                        "assignment/immutable",
                        "cannot assign through an immutable reference",
                    ),
                    _ => self.check_assignment_target(base),
                }
            }
""",
)

marker = """    fn type_of_name(&self, name: ResolvedName) -> Ty {
"""
helpers = r'''    fn place_type(&self, expr: &HirExpr) -> Ty {
        match &expr.kind {
            HirExprKind::Name { reference } => self.type_of_name(reference.root),
            HirExprKind::Member { base, name } => match self.env.lookup_member(&self.place_type(base), name) {
                MemberLookup::Field(ty) => ty,
                _ => Ty::Unknown,
            },
            HirExprKind::Index { base, .. } => match self.place_type(base) {
                Ty::Array { element } | Ty::Slice { element, .. } => *element,
                Ty::Reference { inner, .. } => match *inner {
                    Ty::Array { element } | Ty::Slice { element, .. } => *element,
                    _ => Ty::Unknown,
                },
                _ => Ty::Unknown,
            },
            HirExprKind::Unary { op, value } => match op {
                UnaryOp::AddressOf => Ty::Reference {
                    mutable: false,
                    inner: Box::new(self.place_type(value)),
                },
                UnaryOp::AddressOfMut => Ty::Reference {
                    mutable: true,
                    inner: Box::new(self.place_type(value)),
                },
                UnaryOp::Deref => match self.place_type(value) {
                    Ty::Pointer { inner, .. } | Ty::Reference { inner, .. } => *inner,
                    _ => Ty::Unknown,
                },
                _ => Ty::Unknown,
            },
            _ => Ty::Unknown,
        }
    }

    fn check_member(&mut self, span: Span, base_ty: &Ty, name: &str) -> Ty {
        match self.env.lookup_member(base_ty, name) {
            MemberLookup::Field(ty) => ty,
            MemberLookup::MissingField => {
                self.diagnostic(
                    span,
                    "type/unknown-field",
                    format!("type {base_ty:?} has no field `{name}`"),
                );
                Ty::Error
            }
            MemberLookup::Unsupported => {
                self.diagnostic(
                    span,
                    "type/member",
                    format!("member access `{name}` is not valid on {base_ty:?}"),
                );
                Ty::Error
            }
            MemberLookup::Unknown => Ty::Unknown,
        }
    }

    fn check_qualified_variant(&mut self, span: Span, ty: &Ty, name: &str) {
        let Ty::Nominal(id) = ty else {
            return;
        };
        let kind = self.env.types.get(id).map(|info| info.kind.clone());
        match kind {
            Some(TypeInfoKind::Enum(variants)) => {
                if !variants.contains(name) {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown enum variant `{name}`"),
                    );
                }
            }
            Some(TypeInfoKind::Tagged(variants)) => {
                if !variants.contains_key(name) {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown tagged-union variant `{name}`"),
                    );
                }
            }
            _ => self.diagnostic(
                span,
                "type/unknown-variant",
                format!("{ty:?} does not define variants"),
            ),
        }
    }

    fn check_struct_init(
        &mut self,
        span: Span,
        ty: &Ty,
        variant: Option<&str>,
        fields: &[(String, HirExpr)],
    ) {
        let Ty::Nominal(id) = ty else {
            for (_, value) in fields {
                self.check_expr(value, None);
            }
            return;
        };
        let kind = self.env.types.get(id).map(|info| info.kind.clone());
        match kind {
            Some(TypeInfoKind::Struct(defs)) => {
                if let Some(name) = variant {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("struct type does not define variant `{name}`"),
                    );
                }
                self.check_init_fields(span, fields, &defs);
            }
            Some(TypeInfoKind::Tagged(variants)) => {
                let Some(name) = variant else {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        "tagged-union construction requires a qualified variant",
                    );
                    for (_, value) in fields {
                        self.check_expr(value, None);
                    }
                    return;
                };
                if let Some(defs) = variants.get(name) {
                    self.check_init_fields(span, fields, defs);
                } else {
                    self.diagnostic(
                        span,
                        "type/unknown-variant",
                        format!("unknown tagged-union variant `{name}`"),
                    );
                    for (_, value) in fields {
                        self.check_expr(value, None);
                    }
                }
            }
            _ => {
                self.diagnostic(
                    span,
                    "type/constructor",
                    format!("brace construction is not valid for {ty:?}"),
                );
                for (_, value) in fields {
                    self.check_expr(value, None);
                }
            }
        }
    }

    fn check_init_fields(
        &mut self,
        span: Span,
        fields: &[(String, HirExpr)],
        defs: &BTreeMap<String, FieldInfo>,
    ) {
        let mut seen = BTreeSet::new();
        for (name, value) in fields {
            if !seen.insert(name.clone()) {
                self.diagnostic(
                    value.span,
                    "type/duplicate-field",
                    format!("duplicate field `{name}`"),
                );
                continue;
            }
            if let Some(field) = defs.get(name) {
                let actual = self.check_expr(value, Some(&field.ty));
                self.require_assignable(value.span, &field.ty, &actual, "type/mismatch");
            } else {
                self.diagnostic(
                    value.span,
                    "type/unknown-field",
                    format!("unknown field `{name}`"),
                );
                self.check_expr(value, None);
            }
        }
        for (name, field) in defs {
            if !field.has_default && !seen.contains(name) {
                self.diagnostic(
                    span,
                    "type/missing-field",
                    format!("missing required field `{name}`"),
                );
            }
        }
    }

'''
s = replace_once(s, marker, helpers + marker)

# Replace the old binding-only pattern implementation with validation + exact binding types.
start = s.index("    fn bind_pattern_type(&mut self, pattern: &HirPattern, ty: &Ty) {")
end = s.index("    fn require_assignable(", start)
new_pattern_impl = r'''    fn check_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
        let mut bindings = BTreeMap::new();
        self.collect_pattern_bindings(pattern, ty, &mut bindings);
        for (local, binding_ty) in bindings {
            self.local_types.insert(local, binding_ty);
        }
    }

    fn collect_pattern_bindings(
        &mut self,
        pattern: &HirPattern,
        ty: &Ty,
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        match &pattern.kind {
            HirPatternKind::Binding { local, .. } => {
                out.insert(*local, ty.clone());
            }
            HirPatternKind::As { local, pattern } => {
                out.insert(*local, ty.clone());
                self.collect_pattern_bindings(pattern, ty, out);
            }
            HirPatternKind::Literal { value } => {
                self.check_pattern_literal(pattern.span, value, ty);
            }
            HirPatternKind::Range { start, end, .. } => {
                self.check_pattern_literal(pattern.span, start, ty);
                self.check_pattern_literal(pattern.span, end, ty);
                if !matches!(ty, Ty::Unknown | Ty::Error | Ty::Int { .. } | Ty::Char) {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!("range pattern requires an integer or char scrutinee, found {ty:?}"),
                    );
                }
            }
            HirPatternKind::Some { value } => match ty {
                Ty::Optional { inner } => self.collect_pattern_bindings(value, inner, out),
                Ty::Unknown | Ty::Error => {
                    self.collect_pattern_bindings(value, &Ty::Unknown, out)
                }
                _ => {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!("Some(...) pattern requires an optional scrutinee, found {ty:?}"),
                    );
                    self.collect_pattern_bindings(value, &Ty::Unknown, out);
                }
            },
            HirPatternKind::None { .. } => {
                if !matches!(ty, Ty::Optional { .. } | Ty::Unknown | Ty::Error) {
                    self.diagnostic(
                        pattern.span,
                        "pattern/type",
                        format!("None pattern requires an optional scrutinee, found {ty:?}"),
                    );
                }
            }
            HirPatternKind::Sequence { items, rest } => {
                let element = match ty {
                    Ty::Array { element } | Ty::Slice { element, .. } => element.as_ref().clone(),
                    Ty::Unknown | Ty::Error => Ty::Unknown,
                    _ => {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("sequence pattern requires an array or slice, found {ty:?}"),
                        );
                        Ty::Unknown
                    }
                };
                for item in items {
                    self.collect_pattern_bindings(item, &element, out);
                }
                if let Some(id) = rest {
                    out.insert(*id, ty.clone());
                }
            }
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                self.require_pattern_type(pattern.span, &expected, ty);
                let defs = match &expected {
                    Ty::Nominal(id) => match self.env.types.get(id).map(|info| info.kind.clone()) {
                        Some(TypeInfoKind::Struct(fields)) => Some(fields),
                        _ => None,
                    },
                    _ => None,
                };
                if let Some(defs) = defs {
                    self.collect_record_pattern_fields(pattern.span, fields, &defs, out);
                } else {
                    if !matches!(expected, Ty::Unknown | Ty::Error) {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("struct pattern requires a struct type, found {expected:?}"),
                        );
                    }
                    self.collect_unknown_pattern_fields(fields, out);
                }
            }
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                self.require_pattern_type(pattern.span, &expected, ty);
                let kind = match &expected {
                    Ty::Nominal(id) => self.env.types.get(id).map(|info| info.kind.clone()),
                    _ => None,
                };
                match kind {
                    Some(TypeInfoKind::Enum(variants)) => {
                        if !variants.contains(name) {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-variant",
                                format!("unknown enum variant `{name}`"),
                            );
                        }
                        if !fields.is_empty() {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-field",
                                format!("enum variant `{name}` has no payload fields"),
                            );
                            self.collect_unknown_pattern_fields(fields, out);
                        }
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        if let Some(defs) = variants.get(name) {
                            self.collect_record_pattern_fields(pattern.span, fields, defs, out);
                        } else {
                            self.diagnostic(
                                pattern.span,
                                "pattern/unknown-variant",
                                format!("unknown tagged-union variant `{name}`"),
                            );
                            self.collect_unknown_pattern_fields(fields, out);
                        }
                    }
                    Some(_) => {
                        self.diagnostic(
                            pattern.span,
                            "pattern/type",
                            format!("variant pattern requires an enum or tagged union, found {expected:?}"),
                        );
                        self.collect_unknown_pattern_fields(fields, out);
                    }
                    None => self.collect_unknown_pattern_fields(fields, out),
                }
            }
            HirPatternKind::Map { entries, .. } => {
                // Map-pattern typing depends on the collection pattern protocol, which is
                // intentionally not modeled in this primitive type environment yet.
                for entry in entries {
                    out.insert(entry.local, Ty::Unknown);
                }
            }
            HirPatternKind::Or { patterns } => {
                let mut merged: Option<BTreeMap<LocalId, Ty>> = None;
                for alternative in patterns {
                    let mut branch = BTreeMap::new();
                    self.collect_pattern_bindings(alternative, ty, &mut branch);
                    if let Some(current) = &mut merged {
                        let current_ids = current.keys().copied().collect::<BTreeSet<_>>();
                        let branch_ids = branch.keys().copied().collect::<BTreeSet<_>>();
                        if current_ids != branch_ids {
                            self.diagnostic(
                                alternative.span,
                                "pattern/or-bindings",
                                "OR-pattern alternatives must bind the same locals",
                            );
                        }
                        for (local, branch_ty) in branch {
                            match current.get(&local).cloned() {
                                Some(current_ty)
                                    if !matches!(current_ty, Ty::Unknown | Ty::Error)
                                        && !matches!(branch_ty, Ty::Unknown | Ty::Error)
                                        && current_ty != branch_ty =>
                                {
                                    self.diagnostic(
                                        alternative.span,
                                        "pattern/or-binding-type",
                                        format!(
                                            "OR-pattern binding {local:?} has incompatible types {current_ty:?} and {branch_ty:?}"
                                        ),
                                    );
                                }
                                Some(current_ty)
                                    if matches!(current_ty, Ty::Unknown | Ty::Error)
                                        && !matches!(branch_ty, Ty::Unknown | Ty::Error) =>
                                {
                                    current.insert(local, branch_ty);
                                }
                                None => {
                                    current.insert(local, branch_ty);
                                }
                                _ => {}
                            }
                        }
                    } else {
                        merged = Some(branch);
                    }
                }
                if let Some(merged) = merged {
                    out.extend(merged);
                }
            }
            HirPatternKind::Wildcard => {}
        }
    }

    fn collect_record_pattern_fields(
        &mut self,
        span: Span,
        fields: &[crate::body_hir::HirPatternField],
        defs: &BTreeMap<String, FieldInfo>,
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        let mut seen = BTreeSet::new();
        for field in fields {
            if !seen.insert(field.name.clone()) {
                self.diagnostic(
                    span,
                    "pattern/duplicate-field",
                    format!("duplicate pattern field `{}`", field.name),
                );
                continue;
            }
            let Some(info) = defs.get(&field.name) else {
                self.diagnostic(
                    span,
                    "pattern/unknown-field",
                    format!("unknown pattern field `{}`", field.name),
                );
                if let Some(pattern) = &field.pattern {
                    self.collect_pattern_bindings(pattern, &Ty::Unknown, out);
                }
                if let Some(local) = field.shorthand_local {
                    out.insert(local, Ty::Unknown);
                }
                continue;
            };
            if let Some(pattern) = &field.pattern {
                self.collect_pattern_bindings(pattern, &info.ty, out);
            }
            if let Some(local) = field.shorthand_local {
                out.insert(local, info.ty.clone());
            }
        }
    }

    fn collect_unknown_pattern_fields(
        &mut self,
        fields: &[crate::body_hir::HirPatternField],
        out: &mut BTreeMap<LocalId, Ty>,
    ) {
        for field in fields {
            if let Some(pattern) = &field.pattern {
                self.collect_pattern_bindings(pattern, &Ty::Unknown, out);
            }
            if let Some(local) = field.shorthand_local {
                out.insert(local, Ty::Unknown);
            }
        }
    }

    fn check_pattern_literal(&mut self, span: Span, value: &ast::PatternLiteral, ty: &Ty) {
        if matches!(ty, Ty::Unknown | Ty::Error) {
            return;
        }
        let literal_ty = pattern_literal_ty(value);
        if !self.is_assignable(ty, &literal_ty) {
            self.diagnostic(
                span,
                "pattern/type",
                format!("pattern literal {literal_ty:?} is incompatible with {ty:?}"),
            );
        }
    }

    fn require_pattern_type(&mut self, span: Span, expected: &Ty, actual: &Ty) {
        if matches!(expected, Ty::Unknown | Ty::Error) || matches!(actual, Ty::Unknown | Ty::Error) {
            return;
        }
        if expected != actual {
            self.diagnostic(
                span,
                "pattern/type",
                format!("pattern expects {expected:?}, found scrutinee {actual:?}"),
            );
        }
    }

'''
s = s[:start] + new_pattern_impl + s[end:]

# Add pattern literal type helper alongside literal parsing helpers.
marker = """fn integer_literal_ty(text: &str) -> Ty {
"""
pattern_literal_helper = r'''fn pattern_literal_ty(value: &ast::PatternLiteral) -> Ty {
    match value {
        ast::PatternLiteral::Integer { text } => integer_literal_ty(text),
        ast::PatternLiteral::Character { .. } => Ty::Char,
        ast::PatternLiteral::String { .. } => Ty::Str,
        ast::PatternLiteral::Bool { .. } => Ty::Bool,
    }
}

'''
s = replace_once(s, marker, pattern_literal_helper + marker)
p.write_text(s)

# Focused typed-HIR tests.
p = Path("crates/forge-frontend/tests/typecheck.rs")
s = p.read_text()
if "fn struct_member_access_has_exact_type()" not in s:
    s += r'''

#[test]
fn struct_member_access_has_exact_type() {
    let output = check(r#"
        module test.member_type;
        struct Packet { kind: u8; count: u32; }
        fn read_count(p: Packet) -> u32 { return p.count; }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn rejects_unknown_struct_member() {
    let output = check(r#"
        module test.member_missing;
        struct Point { x: i32; }
        fn bad(p: Point) -> i32 { return p.y; }
    "#);
    assert!(has(&output, "type/unknown-field"), "{:?}", output.diagnostics);
}

#[test]
fn struct_initializers_check_field_types_and_required_fields() {
    let wrong = check(r#"
        module test.struct_init_wrong;
        struct Packet { kind: u8; count: u32; }
        fn main() -> i32 {
            val p = Packet{kind: true, count: 1u32};
            return 0;
        }
    "#);
    assert!(has(&wrong, "type/mismatch"), "{:?}", wrong.diagnostics);

    let missing = check(r#"
        module test.struct_init_missing;
        struct Packet { kind: u8; count: u32; }
        fn main() -> i32 {
            val p = Packet{kind: 1u8};
            return 0;
        }
    "#);
    assert!(has(&missing, "type/missing-field"), "{:?}", missing.diagnostics);

    let defaulted = check(r#"
        module test.struct_init_default;
        struct Packet { kind: u8; count: u32 = 0u32; }
        fn main() -> i32 {
            val p = Packet{kind: 1u8};
            return 0;
        }
    "#);
    assert!(defaulted.diagnostics.is_empty(), "{:?}", defaulted.diagnostics);
}

#[test]
fn mutable_reference_member_assignment_is_allowed() {
    let output = check(r#"
        module test.mutable_ref_member;
        struct Point { x: i32; }
        fn set_x(p: &mut Point) -> void {
            p.x = 7i32;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn immutable_reference_member_assignment_is_rejected() {
    let output = check(r#"
        module test.immutable_ref_member;
        struct Point { x: i32; }
        fn set_x(p: &Point) -> void {
            p.x = 7i32;
        }
    "#);
    assert!(has(&output, "assignment/immutable"), "{:?}", output.diagnostics);
}

#[test]
fn struct_pattern_binds_declared_field_types() {
    let output = check(r#"
        module test.struct_pattern_types;
        struct Pair { left: u8; right: u32; }
        fn right(pair: Pair) -> u32 {
            val Pair{left, right} = pair;
            val copy: u32 = right;
            return copy;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().next().unwrap();
    assert!(body.local_types.values().any(|ty| *ty == Ty::Int { signed: false, width: IntWidth::W8 }));
    assert!(body.local_types.values().any(|ty| *ty == Ty::Int { signed: false, width: IntWidth::W32 }));
}

#[test]
fn rejects_struct_pattern_against_wrong_scrutinee_type() {
    let output = check(r#"
        module test.struct_pattern_mismatch;
        struct Point { x: i32; }
        struct Other { x: i32; }
        fn bad(value: Other) -> i32 {
            val Point{x} = value;
            return x;
        }
    "#);
    assert!(has(&output, "pattern/type"), "{:?}", output.diagnostics);
}

#[test]
fn tagged_pattern_binds_payload_field_type() {
    let output = check(r#"
        module test.tagged_pattern_type;
        tagged Token { Number { value: i64; }, Plus, }
        fn read(token: Token) -> i64 {
            return match (token) {
                Token::Number{value} => value,
                Token::Plus => 0i64,
            };
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn rejects_unknown_pattern_field_and_variant() {
    let field = check(r#"
        module test.pattern_field_missing;
        struct Point { x: i32; }
        fn bad(point: Point) -> i32 {
            val Point{y} = point;
            return 0;
        }
    "#);
    assert!(has(&field, "pattern/unknown-field"), "{:?}", field.diagnostics);

    let variant = check(r#"
        module test.pattern_variant_missing;
        tagged Token { Plus, }
        fn bad(token: Token) -> i32 {
            return match (token) { Token::Minus => 1, _ => 0 };
        }
    "#);
    assert!(has(&variant, "pattern/unknown-variant"), "{:?}", variant.diagnostics);
}

#[test]
fn pattern_literals_and_option_patterns_are_checked_against_scrutinee() {
    let literal = check(r#"
        module test.pattern_literal_mismatch;
        fn bad(value: u32) -> i32 {
            return match (value) { true => 1, _ => 0 };
        }
    "#);
    assert!(has(&literal, "pattern/type"), "{:?}", literal.diagnostics);

    let option = check(r#"
        module test.pattern_option_mismatch;
        fn bad(value: u32) -> i32 {
            return match (value) { Some(x) => i32(x), _ => 0 };
        }
    "#);
    assert!(has(&option, "pattern/type"), "{:?}", option.diagnostics);
}

#[test]
fn or_pattern_bindings_must_have_identical_types() {
    let output = check(r#"
        module test.or_pattern_type;
        tagged Value {
            Count { x: u32; },
            Flag { x: bool; },
        }
        fn bad(value: Value) -> i32 {
            return match (value) {
                Value::Count{x} | Value::Flag{x} => 1,
                _ => 0,
            };
        }
    "#);
    assert!(has(&output, "pattern/or-binding-type"), "{:?}", output.diagnostics);
}

#[test]
fn or_pattern_same_binding_type_is_accepted() {
    let output = check(r#"
        module test.or_pattern_same_type;
        tagged Value {
            Left { x: u32; },
            Right { x: u32; },
        }
        fn read(value: Value) -> u32 {
            return match (value) {
                Value::Left{x} | Value::Right{x} => x,
            };
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}
'''
p.write_text(s)

# Add representative semantic-negative conformance cases.
Path("examples/conformance/negative/12-unknown-struct-member.fg").write_text("""module examples.conformance.unknown_struct_member;
struct Point { x: i32; }
fn bad(point: Point) -> i32 { return point.y; }
""")
Path("examples/conformance/negative/13-missing-struct-field.fg").write_text("""module examples.conformance.missing_struct_field;
struct Point { x: i32; y: i32; }
fn main() -> i32 {
    val point = Point{x: 1i32};
    return 0;
}
""")
Path("examples/conformance/negative/14-pattern-type-mismatch.fg").write_text("""module examples.conformance.pattern_type_mismatch;
struct Point { x: i32; }
struct Other { x: i32; }
fn bad(value: Other) -> i32 {
    val Point{x} = value;
    return x;
}
""")
Path("examples/conformance/negative/15-or-pattern-binding-type.fg").write_text("""module examples.conformance.or_pattern_binding_type;
tagged Value {
    Count { x: u32; },
    Flag { x: bool; },
}
fn bad(value: Value) -> i32 {
    return match (value) {
        Value::Count{x} | Value::Flag{x} => 1,
        _ => 0,
    };
}
""")

p = Path("examples/conformance/suite.fdn")
s = p.read_text()
old = '    {:path #path "negative/11-tail-return-noncall.fg" :kind :negative :expect :control/tail-call-required}\n'
new = old + '''    {:path #path "negative/12-unknown-struct-member.fg" :kind :negative :expect :type/unknown-field}\n    {:path #path "negative/13-missing-struct-field.fg" :kind :negative :expect :type/missing-field}\n    {:path #path "negative/14-pattern-type-mismatch.fg" :kind :negative :expect :pattern/type}\n    {:path #path "negative/15-or-pattern-binding-type.fg" :kind :negative :expect :pattern/or-binding-type}\n'''
if "negative/12-unknown-struct-member.fg" not in s:
    s = replace_once(s, old, new)
p.write_text(s)

p = Path("crates/forge-conformance/src/main.rs")
s = p.read_text()
old = '''        "type/index-on-type",
        "call/duplicate-name",
'''
new = '''        "type/index-on-type",
        "type/unknown-field",
        "type/missing-field",
        "pattern/type",
        "pattern/or-binding-type",
        "call/duplicate-name",
'''
s = replace_once(s, old, new)
p.write_text(s)
