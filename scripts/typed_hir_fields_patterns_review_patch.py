from pathlib import Path


def replace_once(text: str, old: str, new: str) -> str:
    if old not in text:
        raise SystemExit(f"missing replacement anchor:\n{old[:220]}")
    return text.replace(old, new, 1)


p = Path("crates/forge-frontend/src/typecheck_v1.rs")
s = p.read_text()

s = replace_once(
    s,
    """                self.check_pattern(pattern, &final_ty);
                if *mutable {
""",
    """                self.check_irrefutable_binding_pattern(pattern, &final_ty);
                self.check_pattern(pattern, &final_ty);
                if *mutable {
""",
)

marker = """    fn check_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
"""
helpers = r'''    fn check_irrefutable_binding_pattern(&mut self, pattern: &HirPattern, ty: &Ty) {
        if !self.pattern_is_irrefutable(pattern, ty) {
            self.diagnostic(
                pattern.span,
                "pattern/refutable-binding",
                "destructuring declarations require a statically irrefutable pattern",
            );
        }
    }

    fn pattern_is_irrefutable(&self, pattern: &HirPattern, ty: &Ty) -> bool {
        match &pattern.kind {
            HirPatternKind::Wildcard | HirPatternKind::Binding { .. } => true,
            HirPatternKind::As { pattern, .. } => self.pattern_is_irrefutable(pattern, ty),
            HirPatternKind::Struct { path, fields } => {
                let expected = self.env.ty_from_ref(path);
                if !matches!(expected, Ty::Unknown | Ty::Error)
                    && !matches!(ty, Ty::Unknown | Ty::Error)
                    && expected != *ty
                {
                    // The ordinary pattern type diagnostic owns this mismatch.
                    return true;
                }
                let Ty::Nominal(id) = expected else {
                    return true;
                };
                let Some(TypeInfoKind::Struct(defs)) =
                    self.env.types.get(&id).map(|info| &info.kind)
                else {
                    return true;
                };
                fields.iter().all(|field| {
                    let Some(info) = defs.get(&field.name) else {
                        return true;
                    };
                    field
                        .pattern
                        .as_ref()
                        .is_none_or(|nested| self.pattern_is_irrefutable(nested, &info.ty))
                })
            }
            HirPatternKind::Variant {
                namespace,
                name,
                fields,
                ..
            } => {
                let expected = self.env.ty_from_ref(namespace);
                if !matches!(expected, Ty::Unknown | Ty::Error)
                    && !matches!(ty, Ty::Unknown | Ty::Error)
                    && expected != *ty
                {
                    return true;
                }
                let Ty::Nominal(id) = expected else {
                    return false;
                };
                match self.env.types.get(&id).map(|info| &info.kind) {
                    Some(TypeInfoKind::Enum(variants)) => {
                        variants.len() == 1 && variants.contains(name) && fields.is_empty()
                    }
                    Some(TypeInfoKind::Tagged(variants)) => {
                        if variants.len() != 1 {
                            return false;
                        }
                        let Some(defs) = variants.get(name) else {
                            return false;
                        };
                        fields.iter().all(|field| {
                            let Some(info) = defs.get(&field.name) else {
                                return true;
                            };
                            field.pattern.as_ref().is_none_or(|nested| {
                                self.pattern_is_irrefutable(nested, &info.ty)
                            })
                        })
                    }
                    _ => false,
                }
            }
            HirPatternKind::Or { patterns } => patterns
                .iter()
                .any(|branch| self.pattern_is_irrefutable(branch, ty)),
            // Length-sensitive sequence declarations require fixed-array length to be
            // retained in Ty. Until then, sequence declarations cannot be proven safe.
            HirPatternKind::Sequence { .. }
            | HirPatternKind::Map { .. }
            | HirPatternKind::Literal { .. }
            | HirPatternKind::Range { .. }
            | HirPatternKind::None { .. }
            | HirPatternKind::Some { .. } => false,
        }
    }

'''
s = replace_once(s, marker, helpers + marker)
p.write_text(s)

p = Path("crates/forge-frontend/tests/typecheck.rs")
s = p.read_text()
if "fn nested_struct_members_keep_exact_types()" not in s:
    s += r'''

#[test]
fn nested_struct_members_keep_exact_types() {
    let output = check(r#"
        module test.nested_member_type;
        struct Inner { value: u16; }
        struct Outer { inner: Inner; }
        fn read(outer: Outer) -> u16 { return outer.inner.value; }
    "#);
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
}

#[test]
fn direct_struct_member_assignment_tracks_root_mutability() {
    let mutable = check(r#"
        module test.mutable_struct_member;
        struct Point { x: i32; }
        fn main() -> i32 {
            var point = Point{x: 1i32};
            point.x = 2i32;
            return point.x;
        }
    "#);
    assert!(mutable.diagnostics.is_empty(), "{:?}", mutable.diagnostics);

    let immutable = check(r#"
        module test.immutable_struct_member;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32};
            point.x = 2i32;
            return point.x;
        }
    "#);
    assert!(has(&immutable, "assignment/immutable"), "{:?}", immutable.diagnostics);
}

#[test]
fn struct_and_tagged_constructors_reject_bad_field_sets() {
    let duplicate = check(r#"
        module test.duplicate_struct_field;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32, x: 2i32};
            return 0;
        }
    "#);
    assert!(has(&duplicate, "type/duplicate-field"), "{:?}", duplicate.diagnostics);

    let unknown = check(r#"
        module test.unknown_struct_field;
        struct Point { x: i32; }
        fn main() -> i32 {
            val point = Point{x: 1i32, y: 2i32};
            return 0;
        }
    "#);
    assert!(has(&unknown, "type/unknown-field"), "{:?}", unknown.diagnostics);

    let tagged = check(r#"
        module test.tagged_constructor_fields;
        tagged Token { Number { value: i64; }, Plus, }
        fn main() -> i32 {
            val token = Token::Number{value: true};
            return 0;
        }
    "#);
    assert!(has(&tagged, "type/mismatch"), "{:?}", tagged.diagnostics);
}

#[test]
fn optional_and_sequence_match_bindings_get_element_types() {
    let optional = check(r#"
        module test.optional_pattern_binding;
        fn read(value: u32?) -> u32 {
            return match (value) {
                Some(x) => x,
                None => 0u32,
            };
        }
    "#);
    assert!(optional.diagnostics.is_empty(), "{:?}", optional.diagnostics);

    let sequence = check(r#"
        module test.sequence_pattern_binding;
        fn first(values: u16[]) -> u16 {
            return match (values) {
                [x, ..rest] => x,
                _ => 0u16,
            };
        }
    "#);
    assert!(sequence.diagnostics.is_empty(), "{:?}", sequence.diagnostics);
}

#[test]
fn destructuring_declarations_reject_refutable_patterns() {
    let optional = check(r#"
        module test.refutable_optional_binding;
        fn read(value: u32?) -> u32 {
            val Some(x) = value;
            return x;
        }
    "#);
    assert!(has(&optional, "pattern/refutable-binding"), "{:?}", optional.diagnostics);

    let slice = check(r#"
        module test.refutable_slice_binding;
        fn read(values: u32[]) -> u32 {
            val [first, ..rest] = values;
            return first;
        }
    "#);
    assert!(has(&slice, "pattern/refutable-binding"), "{:?}", slice.diagnostics);
}
'''
p.write_text(s)

Path("examples/conformance/negative/16-refutable-destructuring-binding.fg").write_text("""module examples.conformance.refutable_destructuring_binding;
fn read(value: u32?) -> u32 {
    val Some(x) = value;
    return x;
}
""")

p = Path("examples/conformance/suite.fdn")
s = p.read_text()
old = '    {:path #path "negative/15-or-pattern-binding-type.fg" :kind :negative :expect :pattern/or-binding-type}\n'
new = old + '    {:path #path "negative/16-refutable-destructuring-binding.fg" :kind :negative :expect :pattern/refutable-binding}\n'
if "negative/16-refutable-destructuring-binding.fg" not in s:
    s = replace_once(s, old, new)
p.write_text(s)

p = Path("crates/forge-conformance/src/main.rs")
s = p.read_text()
old = '''        "pattern/type",
        "pattern/or-binding-type",
        "call/duplicate-name",
'''
new = '''        "pattern/type",
        "pattern/or-binding-type",
        "pattern/refutable-binding",
        "call/duplicate-name",
'''
s = replace_once(s, old, new)
p.write_text(s)
