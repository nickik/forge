from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    s = p.read_text()
    if old not in s:
        raise SystemExit(f"anchor not found in {path}: {old[:120]!r}")
    p.write_text(s.replace(old, new, 1))


tc = "crates/forge-frontend/src/typecheck_v1.rs"
fir = "crates/forge-frontend/src/fir_v1.rs"
lib = "crates/forge-frontend/src/lib.rs"
tct = "crates/forge-frontend/tests/typecheck.rs"
firt = "crates/forge-frontend/tests/fir.rs"
plan = "docs/fir-completion-plan.md"

# Semantic protocol and match nodes.
replace_once(tc,
'''    Length {
        count: u64,
        at_least: bool,
    },
}''',
'''    Length {
        count: u64,
        at_least: bool,
    },
    CollectionHasOnly {
        operation: DefId,
        keys: Vec<String>,
    },
}''')

replace_once(tc,
'''    Rest { start: u64, ty: Ty },
}''',
'''    Rest { start: u64, ty: Ty },
    CollectionLookup {
        operation: DefId,
        key: String,
        ty: Ty,
    },
}''')

replace_once(tc,
'''pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub finite_cases: Option<Vec<String>>,
    pub arms: Vec<TypedMatchArmPlan>,
}
''',
'''pub struct TypedMatchPlan {
    pub scrutinee_type: Ty,
    pub finite_cases: Option<Vec<String>>,
    pub arms: Vec<TypedMatchArmPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypedCollectionPatternProtocol {
    pub collection: Ty,
    pub lookup: DefId,
    pub has_only: DefId,
    pub value: Ty,
}
''')

# Resolve the structural collection protocol above FIR.
replace_once(tc,
'''    fn channel_payload(&self, ty: &Ty) -> Option<Ty> {
''',
'''    fn collection_pattern_protocol(&self, ty: &Ty) -> Option<TypedCollectionPatternProtocol> {
        let collection = match ty {
            Ty::Reference { inner, .. } => inner.as_ref().clone(),
            other => other.clone(),
        };
        let Ty::Nominal(id) = collection else {
            return None;
        };
        let lookup = *self.methods.get(&(id, "pattern_get".to_owned()))?;
        let has_only = *self.methods.get(&(id, "pattern_has_only".to_owned()))?;
        let lookup_sig = self.functions.get(&lookup)?;
        let has_only_sig = self.functions.get(&has_only)?;
        let expected_self = Ty::Reference {
            mutable: false,
            inner: Box::new(Ty::Nominal(id)),
        };
        if lookup_sig.params.len() != 2
            || lookup_sig.params[0].name != "self"
            || lookup_sig.params[0].ty != expected_self
            || lookup_sig.params[1].ty != Ty::Str
        {
            return None;
        }
        let Ty::Optional { inner: value } = &lookup_sig.result else {
            return None;
        };
        if matches!(value.as_ref(), Ty::Unknown | Ty::Error | Ty::Void) {
            return None;
        }
        let keys_ty = Ty::Slice {
            mutable: false,
            element: Box::new(Ty::Str),
        };
        if has_only_sig.params.len() != 2
            || has_only_sig.params[0].name != "self"
            || has_only_sig.params[0].ty != expected_self
            || has_only_sig.params[1].ty != keys_ty
            || has_only_sig.result != Ty::Bool
        {
            return None;
        }
        Some(TypedCollectionPatternProtocol {
            collection: Ty::Nominal(id),
            lookup,
            has_only,
            value: value.as_ref().clone(),
        })
    }

    fn channel_payload(&self, ty: &Ty) -> Option<Ty> {
''')

# Type map bindings from the resolved protocol, including optional entry semantics.
replace_once(tc,
'''            HirPatternKind::Map { entries, .. } => {
                // Map-pattern typing depends on the collection pattern protocol, which is
                // intentionally not modeled in this primitive type environment yet.
                for entry in entries {
                    out.insert(entry.local, Ty::Unknown);
                }
            }
''',
'''            HirPatternKind::Map { entries, .. } => {
                let Some(protocol) = self.env.collection_pattern_protocol(ty) else {
                    self.diagnostic(
                        pattern.span,
                        "pattern/collection-protocol",
                        format!(
                            "map pattern requires `pattern_get(self: &T, key: str) -> V?` and `pattern_has_only(self: &T, keys: str[]) -> bool` on {ty:?}"
                        ),
                    );
                    for entry in entries {
                        out.insert(entry.local, Ty::Error);
                    }
                    return;
                };
                let mut seen = BTreeSet::new();
                for entry in entries {
                    if !seen.insert(entry.keyword.clone()) {
                        self.diagnostic(
                            pattern.span,
                            "pattern/duplicate-key",
                            format!("map pattern key `:{}` appears more than once", entry.keyword),
                        );
                    }
                    let binding_ty = if entry.optional {
                        Ty::Optional {
                            inner: Box::new(protocol.value.clone()),
                        }
                    } else {
                        protocol.value.clone()
                    };
                    out.insert(entry.local, binding_ty);
                }
            }
''')

# A nominal collection with the protocol is now a planned match domain.
replace_once(tc,
'''                Some(TypeInfoKind::Struct(_))
                    | Some(TypeInfoKind::Enum(_))
                    | Some(TypeInfoKind::Tagged(_))
            ),
''',
'''                Some(TypeInfoKind::Struct(_))
                    | Some(TypeInfoKind::Enum(_))
                    | Some(TypeInfoKind::Tagged(_))
            ) || self.env.collection_pattern_protocol(ty).is_some(),
''')

replace_once(tc,
'''            HirPatternKind::None { .. } => None,
            HirPatternKind::Map { .. } => None,
''',
'''            HirPatternKind::None { .. } => None,
            HirPatternKind::Map {
                entries,
                ignore_rest,
            } => {
                let protocol = self.env.collection_pattern_protocol(ty)?;
                let optional_ty = Ty::Optional {
                    inner: Box::new(protocol.value.clone()),
                };
                let mut alternatives = one(MatchCondition::Always, Vec::new());
                if !*ignore_rest {
                    let keys = entries
                        .iter()
                        .map(|entry| entry.keyword.clone())
                        .collect::<Vec<_>>();
                    alternatives = self.combine_match_alternatives(
                        alternatives,
                        one(
                            test(MatchTest::CollectionHasOnly {
                                operation: protocol.has_only,
                                keys,
                            }),
                            Vec::new(),
                        ),
                    );
                }
                for entry in entries {
                    let mut lookup = projections.to_vec();
                    lookup.push(MatchProjection::CollectionLookup {
                        operation: protocol.lookup,
                        key: entry.keyword.clone(),
                        ty: optional_ty.clone(),
                    });
                    if entry.optional {
                        for alternative in &mut alternatives {
                            alternative.bindings.push(TypedMatchBinding {
                                local: entry.local,
                                ty: optional_ty.clone(),
                                projections: lookup.clone(),
                            });
                        }
                    } else {
                        alternatives = self.combine_match_alternatives(
                            alternatives,
                            one(
                                MatchCondition::Test {
                                    projections: lookup.clone(),
                                    test: MatchTest::OptionSome,
                                },
                                Vec::new(),
                            ),
                        );
                        let mut payload = lookup;
                        payload.push(MatchProjection::OptionPayload {
                            ty: protocol.value.clone(),
                        });
                        for alternative in &mut alternatives {
                            alternative.bindings.push(TypedMatchBinding {
                                local: entry.local,
                                ty: protocol.value.clone(),
                                projections: payload.clone(),
                            });
                        }
                    }
                }
                Some(alternatives)
            }
''')

# Collection tests are non-finite but participate in usefulness through exact equality.
replace_once(tc,
'''                    MatchTest::ScalarLiteral { .. }
                    | MatchTest::ScalarRange { .. }
                    | MatchTest::Length { .. } => None,
''',
'''                    MatchTest::ScalarLiteral { .. }
                    | MatchTest::ScalarRange { .. }
                    | MatchTest::Length { .. }
                    | MatchTest::CollectionHasOnly { .. } => None,
''')

# FIR semantic operations for protocol queries.
replace_once(fir,
'''    Subsequence {
        base: FirValueId,
        start: u64,
    },
''',
'''    Subsequence {
        base: FirValueId,
        start: u64,
    },
    CollectionPatternLookup {
        collection: FirValueId,
        operation: DefId,
        key: String,
    },
    CollectionPatternHasOnly {
        collection: FirValueId,
        operation: DefId,
        keys: Vec<String>,
    },
''')

# Projection result type now includes collection lookup.
replace_once(fir,
'''                        MatchProjection::OptionPayload { ty }
                        | MatchProjection::Field { ty, .. }
                        | MatchProjection::Index { ty, .. }
                        | MatchProjection::Rest { ty, .. } => ty.clone(),
''',
'''                        MatchProjection::OptionPayload { ty }
                        | MatchProjection::Field { ty, .. }
                        | MatchProjection::Index { ty, .. }
                        | MatchProjection::Rest { ty, .. }
                        | MatchProjection::CollectionLookup { ty, .. } => ty.clone(),
''')

# Exact-key collection test is an explicit FIR protocol query.
replace_once(fir,
'''            MatchTest::Length { count, at_least } => {
''',
'''            MatchTest::CollectionHasOnly { operation, keys } => self.emit_value(
                span,
                Ty::Bool,
                FirInstructionKind::CollectionPatternHasOnly {
                    collection: value,
                    operation: *operation,
                    keys: keys.clone(),
                },
            ),
            MatchTest::Length { count, at_least } => {
''')

# Lookup projections stay target-independent and retain the resolved method DefId.
replace_once(fir,
'''                MatchProjection::Rest { start, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::Subsequence {
                        base: value,
                        start: *start,
                    },
                ),
''',
'''                MatchProjection::Rest { start, ty } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::Subsequence {
                        base: value,
                        start: *start,
                    },
                ),
                MatchProjection::CollectionLookup {
                    operation,
                    key,
                    ty,
                } => self.emit_value(
                    span,
                    ty.clone(),
                    FirInstructionKind::CollectionPatternLookup {
                        collection: value,
                        operation: *operation,
                        key: key.clone(),
                    },
                ),
''')

# Public semantic surface.
replace_once(lib,
'''    TypeCheckOutput, TypeDiagnostic, TypedBitField, TypedBitFieldAccess, TypedBitStruct, TypedBody,
    TypedCapture, TypedClosurePlan, TypedContextOverride, TypedContextScope, TypedExpr,
''',
'''    TypeCheckOutput, TypeDiagnostic, TypedBitField, TypedBitFieldAccess, TypedBitStruct, TypedBody,
    TypedCapture, TypedClosurePlan, TypedCollectionPatternProtocol, TypedContextOverride,
    TypedContextScope, TypedExpr,
''')

# Tests.
Path(tct).write_text(Path(tct).read_text() + r'''

#[test]
fn map_pattern_protocol_types_required_and_optional_bindings() {
    let output = check(
        r#"
        module test.map_protocol;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name, :age age?, ..} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let body = output.functions.values().find(|body| body.return_type == Ty::Int { signed: false, width: IntWidth::W32 }).unwrap();
    assert!(body.local_types.values().any(|ty| *ty == Ty::Optional { inner: Box::new(Ty::Int { signed: false, width: IntWidth::W32 }) }));
}

#[test]
fn map_pattern_requires_complete_collection_protocol() {
    let output = check(
        r#"
        module test.map_protocol_missing;
        struct Dict {}
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name, ..} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(has(&output, "pattern/collection-protocol"), "{:?}", output.diagnostics);
}

#[test]
fn map_pattern_rejects_duplicate_keys() {
    let output = check(
        r#"
        module test.map_duplicate;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name first, :name second, ..} => first,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(has(&output, "pattern/duplicate-key"), "{:?}", output.diagnostics);
}
''')

Path(firt).write_text(Path(firt).read_text() + r'''

#[test]
fn map_pattern_lowers_resolved_collection_protocol_operations() {
    let output = lower(
        r#"
        module test.fir_map_protocol;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> u32 {
            return match (map) {
                {:name name} => name,
                _ => 0u32,
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::CollectionPatternLookup { key, .. } if key == "name"
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::CollectionPatternHasOnly { keys, .. } if keys == &vec!["name".to_owned()]
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionIsSome { .. })));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::OptionUnwrap { .. })));
}

#[test]
fn map_pattern_rest_skips_closed_key_check_and_optional_binds_option() {
    let output = lower(
        r#"
        module test.fir_map_rest;
        struct Dict {}
        impl Dict {
            fn pattern_get(self: &Dict, key: str) -> u32? { return None; }
            fn pattern_has_only(self: &Dict, keys: str[]) -> bool { return true; }
        }
        fn use_map(map: Dict) -> void {
            match (map) {
                {:age age?, ..} => {},
                _ => {},
            };
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::CollectionPatternLookup { key, .. } if key == "age"
    )));
    assert!(!instructions(&output).any(|op| matches!(op, FirInstructionKind::CollectionPatternHasOnly { .. })));
}
''')

# Completion plan.
replace_once(plan,
'''- Steps 13-16 intentionally untouched.''',
'''- Step 13 complete: bitstruct storage/layout is materialized above FIR; field reads/writes lower through explicit checked mask/shift operations.
- Step 14 complete: map patterns require a resolved nominal collection protocol (`pattern_get` + `pattern_has_only`); required/optional keyword bindings and closed/rest semantics lower through explicit FIR collection-pattern operations.
- Steps 15-16 intentionally untouched.''')

Path(plan).write_text(Path(plan).read_text() + r'''

## Step 14 acceptance tests

- A map-pattern scrutinee must be a nominal type implementing `pattern_get(self: &T, key: str) -> V?` and `pattern_has_only(self: &T, keys: str[]) -> bool`; protocol lookup is resolved above FIR.
- Required keyword entries test lookup presence and bind the unwrapped `V`; optional entries always match and bind `V?`.
- Duplicate keyword entries are rejected semantically.
- A closed map pattern (no `..`) emits an allowed-keys-only protocol test; `..` deliberately skips that test.
- Match plans retain resolved method `DefId`s and concrete value/optional types; no `Ty::Unknown` collection binding reaches successful FIR lowering.
- FIR emits explicit `CollectionPatternLookup` and `CollectionPatternHasOnly` operations and never re-runs method/protocol resolution.
''')
