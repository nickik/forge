from pathlib import Path

p = Path('crates/forge-frontend/src/resolution_v1.rs')
s = p.read_text()
old = """    let mut output = BodyResolutionOutput::default();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let owner = DefId(index as u32);
        let mut resolver = Resolver::new(owner, module, &imports, &mut output.diagnostics);
"""
new = """    let qualified_only_variants = source
        .declarations
        .iter()
        .flat_map(|declaration| match &declaration.kind.kind {
            DeclKind::Enum(value) => value
                .variants
                .iter()
                .map(|variant| variant.name.clone())
                .collect::<Vec<_>>(),
            DeclKind::Tagged(value) => value
                .variants
                .iter()
                .map(|variant| variant.name.clone())
                .collect::<Vec<_>>(),
            _ => Vec::new(),
        })
        .collect::<BTreeSet<_>>();

    let mut output = BodyResolutionOutput::default();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let owner = DefId(index as u32);
        let mut resolver = Resolver::new(
            owner,
            module,
            &imports,
            &qualified_only_variants,
            &mut output.diagnostics,
        );
"""
assert old in s
s = s.replace(old, new, 1)

old = """    imports: &'a BTreeMap<String, u32>,
    diagnostics: &'d mut Vec<HirDiagnostic>,
"""
new = """    imports: &'a BTreeMap<String, u32>,
    qualified_only_variants: &'a BTreeSet<String>,
    diagnostics: &'d mut Vec<HirDiagnostic>,
"""
assert old in s
s = s.replace(old, new, 1)

old = """        imports: &'a BTreeMap<String, u32>,
        diagnostics: &'d mut Vec<HirDiagnostic>,
"""
new = """        imports: &'a BTreeMap<String, u32>,
        qualified_only_variants: &'a BTreeSet<String>,
        diagnostics: &'d mut Vec<HirDiagnostic>,
"""
assert old in s
s = s.replace(old, new, 1)

old = """            module,
            imports,
            diagnostics,
"""
new = """            module,
            imports,
            qualified_only_variants,
            diagnostics,
"""
assert old in s
s = s.replace(old, new, 1)

old = """        if let Some(symbol) = self.module.symbols.get(name).and_then(|set| set.value_def) {
            self.record_use(name, span, ResolvedName::Def(symbol));
            return;
        }
        if let Some(index) = self.imports.get(name).copied() {
"""
new = """        if let Some(symbol) = self.module.symbols.get(name).and_then(|set| set.value_def) {
            self.record_use(name, span, ResolvedName::Def(symbol));
            return;
        }
        // Preserve type identity in expression position so semantic analysis can
        // diagnose misuse precisely instead of losing it as an unresolved name.
        if let Some(symbol) = self.module.symbols.get(name).and_then(|set| set.type_def) {
            self.record_use(name, span, ResolvedName::Def(symbol));
            return;
        }
        if builtin_types().contains(name) {
            self.record_use(name, span, ResolvedName::BuiltinType);
            return;
        }
        if let Some(index) = self.imports.get(name).copied() {
"""
assert old in s
s = s.replace(old, new, 1)

old = """        if matches!(name, \"Some\") {
            self.record_use(name, span, ResolvedName::BuiltinValue);
            return;
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!(\"unresolved value name `{name}`\"),
        });
"""
new = """        if matches!(name, \"Some\") {
            self.record_use(name, span, ResolvedName::BuiltinValue);
            return;
        }
        if self.qualified_only_variants.contains(name) {
            self.diagnostics.push(HirDiagnostic {
                span,
                message: format!(
                    \"name/qualified-variant-required: variant `{name}` must be qualified by its enum/tagged type\"
                ),
            });
            return;
        }
        self.diagnostics.push(HirDiagnostic {
            span,
            message: format!(\"unresolved value name `{name}`\"),
        });
"""
assert old in s
s = s.replace(old, new, 1)
p.write_text(s)

p = Path('crates/forge-frontend/src/typecheck_v1.rs')
s = p.read_text()
old = """    local_types: BTreeMap<LocalId, Ty>,
    expressions: Vec<TypedExpr>,
"""
new = """    local_types: BTreeMap<LocalId, Ty>,
    mutable_locals: BTreeSet<LocalId>,
    expressions: Vec<TypedExpr>,
"""
assert old in s
s = s.replace(old, new, 1)
old = """            local_types: BTreeMap::new(),
            expressions: Vec::new(),
"""
new = """            local_types: BTreeMap::new(),
            mutable_locals: BTreeSet::new(),
            expressions: Vec::new(),
"""
assert old in s
s = s.replace(old, new, 1)
old = """            HirStmtKind::Value {
                pattern, ty, value, ..
            } => {
"""
new = """            HirStmtKind::Value {
                mutable,
                pattern,
                ty,
                value,
            } => {
"""
assert old in s
s = s.replace(old, new, 1)
old = """                self.bind_pattern_type(pattern, &final_ty);
            }
            HirStmtKind::Assignment { target, value } => {
                let target_ty = self.check_expr(target, None);
"""
new = """                self.bind_pattern_type(pattern, &final_ty);
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
            }
            HirStmtKind::Assignment { target, value } => {
                self.check_assignment_target(target);
                let target_ty = self.check_expr(target, None);
"""
assert old in s
s = s.replace(old, new, 1)
old = """            HirStmtKind::ForEach {
                pattern,
                iterable,
                body,
                ..
            } => {
"""
new = """            HirStmtKind::ForEach {
                mutable,
                pattern,
                iterable,
                body,
            } => {
"""
assert old in s
s = s.replace(old, new, 1)
old = """                self.bind_pattern_type(pattern, &element);
                self.check_block(body);
"""
new = """                self.bind_pattern_type(pattern, &element);
                if *mutable {
                    self.mark_pattern_mutable(pattern);
                }
                self.check_block(body);
"""
assert old in s
s = s.replace(old, new, 1)
old = """            HirExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, None);
                self.check_expr(index, None);
                match base_ty {
"""
new = """            HirExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, None);
                if self.is_type_expr(index) {
                    self.diagnostic(
                        index.span,
                        \"type/index-on-type\",
                        \"a type cannot be used as an index; Forge v1 does not support generic application syntax\",
                    );
                } else {
                    self.check_expr(index, None);
                }
                match base_ty {
"""
assert old in s
s = s.replace(old, new, 1)
marker = """    fn type_of_name(&self, name: ResolvedName) -> Ty {
"""
insert = """    fn is_type_expr(&self, expr: &HirExpr) -> bool {
        match &expr.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::BuiltinType => true,
                ResolvedName::Def(id) => self.env.types.contains_key(&id),
                _ => false,
            },
            _ => false,
        }
    }

    fn check_assignment_target(&mut self, target: &HirExpr) {
        match &target.kind {
            HirExprKind::Name { reference } => match reference.root {
                ResolvedName::Local(id) => {
                    if !self.mutable_locals.contains(&id) {
                        self.diagnostic(
                            target.span,
                            \"assignment/immutable\",
                            \"cannot assign to a `val` binding\",
                        );
                    }
                }
                _ => self.diagnostic(
                    target.span,
                    \"assignment/invalid-target\",
                    \"assignment target is not a mutable local or writable place\",
                ),
            },
            HirExprKind::Member { base, .. } | HirExprKind::Index { base, .. } => {
                self.check_assignment_target(base);
            }
            HirExprKind::Unary {
                op: UnaryOp::Deref,
                value,
            } => {
                let value_ty = self.check_expr(value, None);
                if matches!(value_ty, Ty::Reference { mutable: false, .. }) {
                    self.diagnostic(
                        target.span,
                        \"assignment/immutable\",
                        \"cannot assign through an immutable reference\",
                    );
                } else if !matches!(
                    value_ty,
                    Ty::Reference { mutable: true, .. } | Ty::Pointer { .. }
                ) {
                    self.diagnostic(
                        target.span,
                        \"assignment/invalid-target\",
                        \"dereference assignment requires a pointer or reference\",
                    );
                }
            }
            _ => self.diagnostic(
                target.span,
                \"assignment/invalid-target\",
                \"expression is not a valid assignment target\",
            ),
        }
    }

"""
assert marker in s
s = s.replace(marker, insert + marker, 1)
marker = """    fn bind_pattern_type(&mut self, pattern: &HirPattern, ty: &Ty) {
"""
insert = """    fn mark_pattern_mutable(&mut self, pattern: &HirPattern) {
        match &pattern.kind {
            HirPatternKind::Binding { local, .. } => {
                self.mutable_locals.insert(*local);
            }
            HirPatternKind::As { local, pattern } => {
                self.mutable_locals.insert(*local);
                self.mark_pattern_mutable(pattern);
            }
            HirPatternKind::Some { value } => self.mark_pattern_mutable(value),
            HirPatternKind::Sequence { items, rest } => {
                for item in items {
                    self.mark_pattern_mutable(item);
                }
                if let Some(id) = rest {
                    self.mutable_locals.insert(*id);
                }
            }
            HirPatternKind::Or { patterns } => {
                for pattern in patterns {
                    self.mark_pattern_mutable(pattern);
                }
            }
            HirPatternKind::Variant { fields, .. } | HirPatternKind::Struct { fields, .. } => {
                for field in fields {
                    if let Some(pattern) = &field.pattern {
                        self.mark_pattern_mutable(pattern);
                    }
                    if let Some(id) = field.shorthand_local {
                        self.mutable_locals.insert(id);
                    }
                }
            }
            HirPatternKind::Map { entries, .. } => {
                for entry in entries {
                    self.mutable_locals.insert(entry.local);
                }
            }
            HirPatternKind::Wildcard
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. } => {}
        }
    }

"""
assert marker in s
s = s.replace(marker, insert + marker, 1)
p.write_text(s)

p = Path('crates/forge-conformance/src/main.rs')
s = p.read_text()
old = """    if expected == \"name/unresolved\" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved.diagnostics.iter().any(|d| d.message.starts_with(\"unresolved \")) { Outcome::Pass }
        else { Outcome::Fail(\"expected unresolved-name diagnostic, but name resolution completed without one\".into()) };
    }

    const IMPLEMENTED_TYPE_CODES: &[&str] = &[
"""
new = """    if expected == \"name/unresolved\" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved.diagnostics.iter().any(|d| d.message.starts_with(\"unresolved \")) { Outcome::Pass }
        else { Outcome::Fail(\"expected unresolved-name diagnostic, but name resolution completed without one\".into()) };
    }

    if expected == \"name/qualified-variant-required\" {
        let resolved = resolve_module_bodies(&ast, &items.module);
        return if resolved
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with(\"name/qualified-variant-required:\"))
        {
            Outcome::Pass
        } else {
            Outcome::Fail(
                \"expected qualified-variant diagnostic, but bare variant was not identified\".into(),
            )
        };
    }

    const IMPLEMENTED_TYPE_CODES: &[&str] = &[
"""
assert old in s
s = s.replace(old, new, 1)
old = """        \"type/return\",
        \"call/duplicate-name\",
"""
new = """        \"type/return\",
        \"type/index-on-type\",
        \"call/duplicate-name\",
"""
assert old in s
s = s.replace(old, new, 1)
p.write_text(s)

Path('examples/conformance/negative/07-type-application-vs-index.fg').write_text("""module examples.conformance.type_application_semantic;
struct Point { x: i32; }
fn identity(x: i32) -> i32 { return x; }
fn main() -> i32 {
    val value: i32 = 1;
    val x = identity[Point](value);
    return 0;
}
""")

p = Path('crates/forge-frontend/tests/typecheck.rs')
s = p.read_text()
append = r'''

#[test]
fn rejects_type_name_as_index_operand() {
    let output = check(r#"
        module test.type_index;
        struct Point { x: i32; }
        fn identity(x: i32) -> i32 { return x; }
        fn main() -> i32 {
            val value: i32 = 1;
            val x = identity[Point](value);
            return 0;
        }
    "#);
    assert!(has(&output, "type/index-on-type"), "{:?}", output.diagnostics);
}

#[test]
fn rejects_assignment_to_val() {
    let output = check(r#"
        module test.immutable_assignment;
        fn main() -> i32 {
            val x: i32 = 1;
            x = 2;
            return x;
        }
    "#);
    assert!(has(&output, "assignment/immutable"), "{:?}", output.diagnostics);
}

#[test]
fn allows_assignment_to_var() {
    let output = check(r#"
        module test.mutable_assignment;
        fn main() -> i32 {
            var x: i32 = 1;
            x = 2;
            return x;
        }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}
'''
if 'fn rejects_type_name_as_index_operand()' not in s:
    s += append
p.write_text(s)

p = Path('crates/forge-frontend/tests/resolution.rs')
s = p.read_text()
append = r'''

#[test]
fn bare_enum_variant_requires_qualification() {
    let parsed = forge_frontend::parse_source(r#"
        module test.bare_variant;
        enum Color { Red, Green, }
        fn main() -> i32 {
            val c: Color = Red;
            return 0;
        }
    "#);
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    let ast = parsed.ast.expect("AST");
    let items = forge_frontend::lower_module(&ast);
    assert!(items.diagnostics.is_empty(), "{:?}", items.diagnostics);
    let resolved = forge_frontend::resolve_module_bodies(&ast, &items.module);
    assert!(
        resolved
            .diagnostics
            .iter()
            .any(|d| d.message.starts_with("name/qualified-variant-required:")),
        "{:?}",
        resolved.diagnostics
    );
}
'''
if 'fn bare_enum_variant_requires_qualification()' not in s:
    s += append
p.write_text(s)
