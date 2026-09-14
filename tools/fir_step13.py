from pathlib import Path


def replace_once(path, old, new):
    p = Path(path)
    text = p.read_text()
    if old not in text:
        raise SystemExit(f"anchor not found in {path}: {old[:120]!r}")
    p.write_text(text.replace(old, new, 1))


typecheck = "crates/forge-frontend/src/typecheck_v1.rs"
fir = "crates/forge-frontend/src/fir_v1.rs"
lib = "crates/forge-frontend/src/lib.rs"
ttype = "crates/forge-frontend/tests/typecheck.rs"
tfir = "crates/forge-frontend/tests/fir.rs"
plan = "docs/fir-completion-plan.md"

# --- typed semantic bitstruct model ---
replace_once(
    typecheck,
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]\npub struct TypedUnsafeScope {\n    pub span: Span,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\n#[serde(tag = "source", rename_all = "snake_case")]\npub enum ResolvedCallArgument {''',
    '''#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]\npub struct TypedUnsafeScope {\n    pub span: Span,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize)]\npub struct TypedBitField {\n    pub name: String,\n    pub offset: u32,\n    pub width: u32,\n    pub ty: Ty,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize)]\npub struct TypedBitStruct {\n    pub owner: DefId,\n    pub storage: Ty,\n    pub storage_bits: u32,\n    pub fields: BTreeMap<String, TypedBitField>,\n}\n\n#[derive(Debug, Clone, PartialEq, Eq, Serialize)]\npub struct TypedBitFieldAccess {\n    pub owner: DefId,\n    pub storage: Ty,\n    pub storage_bits: u32,\n    pub field: TypedBitField,\n}\n\n#[derive(Debug, Clone, PartialEq, Serialize)]\n#[serde(tag = "source", rename_all = "snake_case")]\npub enum ResolvedCallArgument {''')

replace_once(
    typecheck,
    '''    UnsafeOperation {\n        operation: UnsafeOperationKind,\n        provenance: UnsafeProvenance,\n        hir: HirExpr,\n    },\n    OptionalPromote {''',
    '''    UnsafeOperation {\n        operation: UnsafeOperationKind,\n        provenance: UnsafeProvenance,\n        hir: HirExpr,\n    },\n    ResolvedBitField {\n        access: TypedBitFieldAccess,\n        hir: HirExpr,\n    },\n    OptionalPromote {''')

replace_once(
    typecheck,
    '''    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,\n    pub metadata: MetadataTable,''',
    '''    pub enum_values: BTreeMap<DefId, BTreeMap<String, i128>>,\n    pub bitstructs: BTreeMap<DefId, TypedBitStruct>,\n    pub metadata: MetadataTable,''')

replace_once(
    typecheck,
    '''    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),\n    Nominal,''',
    '''    Tagged(BTreeMap<String, BTreeMap<String, FieldInfo>>),\n    BitStruct(TypedBitStruct),\n    Nominal,''')

# Build env with diagnostics so declaration-level layout failures are semantic errors.
replace_once(
    typecheck,
    '''    let env = ModuleTypeEnv::build(source, module, &constant_values, bodies);''',
    '''    let env = ModuleTypeEnv::build(\n        source,\n        module,\n        &constant_values,\n        bodies,\n        &mut output.diagnostics,\n    );\n    output.bitstructs = env\n        .types\n        .iter()\n        .filter_map(|(id, info)| match &info.kind {\n            TypeInfoKind::BitStruct(layout) => Some((*id, layout.clone())),\n            _ => None,\n        })\n        .collect();''')

replace_once(
    typecheck,
    '''        bodies: &BodyHirOutput,\n    ) -> Self {''',
    '''        bodies: &BodyHirOutput,\n        diagnostics: &mut Vec<TypeDiagnostic>,\n    ) -> Self {''')

# Materialize bitstruct layout after aliases/distincts are available.
replace_once(
    typecheck,
    '''                DeclKind::Tagged(value) => {\n                    let variants = value\n                        .variants\n                        .iter()\n                        .map(|variant| {\n                            let fields = variant\n                                .fields\n                                .iter()\n                                .map(|field| {\n                                    (\n                                        field.name.clone(),\n                                        FieldInfo {\n                                            ty: env.lower_ast_type(&field.ty, module),\n                                            has_default: field.default.is_some(),\n                                        },\n                                    )\n                                })\n                                .collect();\n                            (variant.name.clone(), fields)\n                        })\n                        .collect();\n                    env.types.insert(\n                        id,\n                        TypeInfo {\n                            kind: TypeInfoKind::Tagged(variants),\n                        },\n                    );\n                }\n                _ => {}''',
    '''                DeclKind::Tagged(value) => {\n                    let variants = value\n                        .variants\n                        .iter()\n                        .map(|variant| {\n                            let fields = variant\n                                .fields\n                                .iter()\n                                .map(|field| {\n                                    (\n                                        field.name.clone(),\n                                        FieldInfo {\n                                            ty: env.lower_ast_type(&field.ty, module),\n                                            has_default: field.default.is_some(),\n                                        },\n                                    )\n                                })\n                                .collect();\n                            (variant.name.clone(), fields)\n                        })\n                        .collect();\n                    env.types.insert(\n                        id,\n                        TypeInfo {\n                            kind: TypeInfoKind::Tagged(variants),\n                        },\n                    );\n                }\n                DeclKind::BitStruct(value) => {\n                    let storage = env.lower_ast_type(&value.storage, module);\n                    let Some(storage_bits) = unsigned_storage_bits(&storage) else {\n                        diagnostics.push(TypeDiagnostic {\n                            span: declaration.span,\n                            code: "bitstruct/storage".into(),\n                            message: format!(\n                                "bitstruct storage must be u8, u16, u32, or u64; found {storage:?}"\n                            ),\n                        });\n                        env.types.insert(\n                            id,\n                            TypeInfo {\n                                kind: TypeInfoKind::BitStruct(TypedBitStruct {\n                                    owner: id,\n                                    storage,\n                                    storage_bits: 0,\n                                    fields: BTreeMap::new(),\n                                }),\n                            },\n                        );\n                        continue;\n                    };\n                    let mut offset = 0u32;\n                    let mut fields = BTreeMap::new();\n                    for field in &value.fields {\n                        if field.width == 0 {\n                            diagnostics.push(TypeDiagnostic {\n                                span: declaration.span,\n                                code: "bitstruct/width".into(),\n                                message: format!(\n                                    "bitstruct field `{}` must have a non-zero width",\n                                    field.name\n                                ),\n                            });\n                            continue;\n                        }\n                        if fields.contains_key(&field.name) {\n                            diagnostics.push(TypeDiagnostic {\n                                span: declaration.span,\n                                code: "bitstruct/duplicate-field".into(),\n                                message: format!(\n                                    "bitstruct field `{}` is declared more than once",\n                                    field.name\n                                ),\n                            });\n                            continue;\n                        }\n                        let Some(end) = offset.checked_add(field.width) else {\n                            diagnostics.push(TypeDiagnostic {\n                                span: declaration.span,\n                                code: "bitstruct/width".into(),\n                                message: "bitstruct field layout overflows its storage width".into(),\n                            });\n                            continue;\n                        };\n                        if end > storage_bits {\n                            diagnostics.push(TypeDiagnostic {\n                                span: declaration.span,\n                                code: "bitstruct/width".into(),\n                                message: format!(\n                                    "bitstruct field `{}` ends at bit {end}, beyond {storage_bits}-bit storage",\n                                    field.name\n                                ),\n                            });\n                            offset = end;\n                            continue;\n                        }\n                        let ty = bitfield_value_type(field.width);\n                        fields.insert(\n                            field.name.clone(),\n                            TypedBitField {\n                                name: field.name.clone(),\n                                offset,\n                                width: field.width,\n                                ty,\n                            },\n                        );\n                        offset = end;\n                    }\n                    env.types.insert(\n                        id,\n                        TypeInfo {\n                            kind: TypeInfoKind::BitStruct(TypedBitStruct {\n                                owner: id,\n                                storage,\n                                storage_bits,\n                                fields,\n                            }),\n                        },\n                    );\n                }\n                _ => {}''')

# Type identity and member lookup.
replace_once(
    typecheck,
    '''            Some(TypeInfoKind::Distinct(_))\n            | Some(TypeInfoKind::Struct(_))\n            | Some(TypeInfoKind::Enum(_))\n            | Some(TypeInfoKind::Tagged(_))\n            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),''',
    '''            Some(TypeInfoKind::Distinct(_))\n            | Some(TypeInfoKind::Struct(_))\n            | Some(TypeInfoKind::Enum(_))\n            | Some(TypeInfoKind::Tagged(_))\n            | Some(TypeInfoKind::BitStruct(_))\n            | Some(TypeInfoKind::Nominal) => Ty::Nominal(id),''')

replace_once(
    typecheck,
    '''                Some(TypeInfoKind::Struct(fields)) => fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(_) => MemberLookup::Unsupported,''',
    '''                Some(TypeInfoKind::Struct(fields)) => fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(TypeInfoKind::BitStruct(layout)) => layout\n                    .fields\n                    .get(name)\n                    .map(|field| MemberLookup::Field(field.ty.clone()))\n                    .unwrap_or(MemberLookup::MissingField),\n                Some(_) => MemberLookup::Unsupported,''')

replace_once(
    typecheck,
    '''    fn lookup_method(&self, ty: &Ty, name: &str) -> Option<(DefId, &FunctionSig)> {''',
    '''    fn bitfield_access(&self, ty: &Ty, name: &str) -> Option<TypedBitFieldAccess> {\n        let id = match ty {\n            Ty::Reference { inner, .. } => match inner.as_ref() {\n                Ty::Nominal(id) => *id,\n                _ => return None,\n            },\n            Ty::Nominal(id) => *id,\n            _ => return None,\n        };\n        let TypeInfoKind::BitStruct(layout) = &self.types.get(&id)?.kind else {\n            return None;\n        };\n        let field = layout.fields.get(name)?.clone();\n        Some(TypedBitFieldAccess {\n            owner: id,\n            storage: layout.storage.clone(),\n            storage_bits: layout.storage_bits,\n            field,\n        })\n    }\n\n    fn bitstruct_layout(&self, ty: &Ty) -> Option<&TypedBitStruct> {\n        let Ty::Nominal(id) = ty else { return None };\n        match self.types.get(id).map(|info| &info.kind) {\n            Some(TypeInfoKind::BitStruct(layout)) => Some(layout),\n            _ => None,\n        }\n    }\n\n    fn lookup_method(&self, ty: &Ty, name: &str) -> Option<(DefId, &FunctionSig)> {''')

# Expression resolution retains field layout.
replace_once(
    typecheck,
    '''        let mut resolved_context: Option<ContextSlot> = None;\n        let mut resolved_try: Option<(Ty, Ty)> = None;''',
    '''        let mut resolved_context: Option<ContextSlot> = None;\n        let mut resolved_bitfield: Option<TypedBitFieldAccess> = None;\n        let mut resolved_try: Option<(Ty, Ty)> = None;''')

replace_once(
    typecheck,
    '''            HirExprKind::Member { base, name } => {\n                let base_ty = self.check_expr(base, None);\n                self.check_member(expr.span, &base_ty, name)\n            }''',
    '''            HirExprKind::Member { base, name } => {\n                let base_ty = self.check_expr(base, None);\n                if let Some(access) = self.env.bitfield_access(&base_ty, name) {\n                    let ty = access.field.ty.clone();\n                    resolved_bitfield = Some(access);\n                    ty\n                } else {\n                    self.check_member(expr.span, &base_ty, name)\n                }\n            }''')

replace_once(
    typecheck,
    '''        } else if let Some(plan) = resolved_match {\n            TypedExprKind::ResolvedMatch {\n                plan,\n                hir: expr.clone(),\n            }\n        } else if let Some((operation, provenance)) = unsafe_operation {''',
    '''        } else if let Some(plan) = resolved_match {\n            TypedExprKind::ResolvedMatch {\n                plan,\n                hir: expr.clone(),\n            }\n        } else if let Some(access) = resolved_bitfield {\n            TypedExprKind::ResolvedBitField {\n                access,\n                hir: expr.clone(),\n            }\n        } else if let Some((operation, provenance)) = unsafe_operation {''')

# Validate explicit bitstruct storage construction.
replace_once(
    typecheck,
    '''        match &target_ty {\n            Ty::Nominal(id) => {\n                if let Some(underlying) = self.env.distinct_underlying(*id) {\n                    if !self.is_explicitly_convertible(underlying, &source) {\n                        self.diagnostic(\n                            span,\n                            "type/distinct",\n                            format!("cannot construct distinct type from {source:?}"),\n                        );\n                    }\n                }\n            }''',
    '''        match &target_ty {\n            Ty::Nominal(id) => {\n                if let Some(layout) = self.env.bitstruct_layout(&target_ty) {\n                    if !self.is_assignable(&layout.storage, &source) {\n                        self.diagnostic(\n                            span,\n                            "bitstruct/storage-conversion",\n                            format!(\n                                "bitstruct construction requires {:?} storage, found {source:?}",\n                                layout.storage\n                            ),\n                        );\n                    }\n                } else if let Some(underlying) = self.env.distinct_underlying(*id) {\n                    if !self.is_explicitly_convertible(underlying, &source) {\n                        self.diagnostic(\n                            span,\n                            "type/distinct",\n                            format!("cannot construct distinct type from {source:?}"),\n                        );\n                    }\n                }\n            }''')

# Helpers for fixed storage and standard field types.
replace_once(
    typecheck,
    '''fn builtin_ty(name: &str) -> Option<Ty> {''',
    '''fn unsigned_storage_bits(ty: &Ty) -> Option<u32> {\n    match ty {\n        Ty::Int { signed: false, width: IntWidth::W8 } => Some(8),\n        Ty::Int { signed: false, width: IntWidth::W16 } => Some(16),\n        Ty::Int { signed: false, width: IntWidth::W32 } => Some(32),\n        Ty::Int { signed: false, width: IntWidth::W64 } => Some(64),\n        _ => None,\n    }\n}\n\nfn bitfield_value_type(width: u32) -> Ty {\n    if width == 1 {\n        Ty::Bool\n    } else if width <= 8 {\n        Ty::Int { signed: false, width: IntWidth::W8 }\n    } else if width <= 16 {\n        Ty::Int { signed: false, width: IntWidth::W16 }\n    } else if width <= 32 {\n        Ty::Int { signed: false, width: IntWidth::W32 }\n    } else {\n        Ty::Int { signed: false, width: IntWidth::W64 }\n    }\n}\n\nfn builtin_ty(name: &str) -> Option<Ty> {''')

# --- FIR representation/lowering ---
replace_once(
    fir,
    '''        MatchScalar, MatchTest, ResolvedCallArgument, ResolvedReceiver, RuntimeOperationId, Ty,\n        TypeCheckOutput, TypedBody, TypedClosurePlan, TypedExpr, TypedExprKind, TypedMatchBinding,''',
    '''        MatchScalar, MatchTest, ResolvedCallArgument, ResolvedReceiver, RuntimeOperationId, Ty,\n        TypeCheckOutput, TypedBitFieldAccess, TypedBody, TypedClosurePlan, TypedExpr, TypedExprKind, TypedMatchBinding,''')

replace_once(
    fir,
    '''    Convert {\n        value: FirValueId,\n        target: Ty,\n    },\n    MakeArray {''',
    '''    Convert {\n        value: FirValueId,\n        target: Ty,\n    },\n    BitStructStorage {\n        value: FirValueId,\n        storage: Ty,\n    },\n    BitStructFromStorage {\n        value: FirValueId,\n        bitstruct: DefId,\n    },\n    BitFieldCheck {\n        value: FirValueId,\n        width: u32,\n    },\n    MakeArray {''')

# Assignment special-case.
replace_once(
    fir,
    '''            HirStmtKind::Assignment { target, value } => {\n                let value = self.lower_expr(value);\n                if let Some(place) = self.lower_place(target) {\n                    self.emit_void(stmt.span, FirInstructionKind::Store { place, value });\n                }\n            }''',
    '''            HirStmtKind::Assignment { target, value } => {\n                if self.lower_bitfield_assignment(target, value) {\n                    return;\n                }\n                let value = self.lower_expr(value);\n                if let Some(place) = self.lower_place(target) {\n                    self.emit_void(stmt.span, FirInstructionKind::Store { place, value });\n                }\n            }''')

# Resolved field expression dispatch.
replace_once(
    fir,
    '''            TypedExprKind::ResolvedMatch { plan, .. } => self.lower_match(expr, plan, result_ty),\n            TypedExprKind::UnsafeOperation {''',
    '''            TypedExprKind::ResolvedMatch { plan, .. } => self.lower_match(expr, plan, result_ty),\n            TypedExprKind::ResolvedBitField { access, .. } => {\n                self.lower_bitfield_read(expr, access, result_ty)\n            }\n            TypedExprKind::UnsafeOperation {''')

# Bitstruct constructor at typed TypeCall boundary.
replace_once(
    fir,
    '''            HirExprKind::TypeCall { args, .. } => {\n                let Some(value) = first_positional(args) else {\n                    self.diagnostic(\n                        expr.span,\n                        "fir/conversion-arity",\n                        "type conversion requires one positional operand",\n                    );\n                    return self.poison(expr.span, ty);\n                };\n                let value = self.lower_expr(value);\n                self.emit_value(\n                    expr.span,\n                    ty.clone(),\n                    FirInstructionKind::Convert { value, target: ty },\n                )\n            }''',
    '''            HirExprKind::TypeCall { args, .. } => {\n                let Some(value) = first_positional(args) else {\n                    self.diagnostic(\n                        expr.span,\n                        "fir/conversion-arity",\n                        "type conversion requires one positional operand",\n                    );\n                    return self.poison(expr.span, ty);\n                };\n                let value = self.lower_expr(value);\n                if let Ty::Nominal(id) = ty {\n                    if self.all_typed.bitstructs.contains_key(&id) {\n                        return self.emit_value(\n                            expr.span,\n                            Ty::Nominal(id),\n                            FirInstructionKind::BitStructFromStorage {\n                                value,\n                                bitstruct: id,\n                            },\n                        );\n                    }\n                    self.emit_value(\n                        expr.span,\n                        Ty::Nominal(id),\n                        FirInstructionKind::Convert {\n                            value,\n                            target: Ty::Nominal(id),\n                        },\n                    )\n                } else {\n                    self.emit_value(\n                        expr.span,\n                        ty.clone(),\n                        FirInstructionKind::Convert { value, target: ty },\n                    )\n                }\n            }''')

# Insert bitfield lowering before lower_match.
replace_once(
    fir,
    '''    fn lower_match(&mut self, expr: &HirExpr, plan: &TypedMatchPlan, result_ty: Ty) -> FirValueId {''',
    '''    fn lower_bitfield_read(\n        &mut self,\n        expr: &HirExpr,\n        access: &TypedBitFieldAccess,\n        result_ty: Ty,\n    ) -> FirValueId {\n        let HirExprKind::Member { base, .. } = &expr.kind else {\n            self.diagnostic(\n                expr.span,\n                "fir/bitfield-shape",\n                "resolved bit-field access is not attached to a member expression",\n            );\n            return self.poison(expr.span, result_ty);\n        };\n        let base_value = if let Some(place) = self.try_place(base) {\n            self.emit_value(\n                base.span,\n                Ty::Nominal(access.owner),\n                FirInstructionKind::Load { place },\n            )\n        } else {\n            self.lower_expr(base)\n        };\n        let storage = self.emit_value(\n            expr.span,\n            access.storage.clone(),\n            FirInstructionKind::BitStructStorage {\n                value: base_value,\n                storage: access.storage.clone(),\n            },\n        );\n        let shifted = if access.field.offset == 0 {\n            storage\n        } else {\n            let shift = self.emit_integer_const(\n                expr.span,\n                access.storage.clone(),\n                access.field.offset as u64,\n            );\n            self.emit_value(\n                expr.span,\n                access.storage.clone(),\n                FirInstructionKind::Binary {\n                    op: BinaryOp::ShiftRight,\n                    overflow: Some(OverflowMode::Checked),\n                    left: storage,\n                    right: shift,\n                },\n            )\n        };\n        let mask = bit_mask(access.field.width);\n        let mask_value = self.emit_integer_const(expr.span, access.storage.clone(), mask);\n        let masked = self.emit_value(\n            expr.span,\n            access.storage.clone(),\n            FirInstructionKind::Binary {\n                op: BinaryOp::BitAnd,\n                overflow: None,\n                left: shifted,\n                right: mask_value,\n            },\n        );\n        if result_ty == Ty::Bool {\n            let zero = self.emit_integer_const(expr.span, access.storage.clone(), 0);\n            self.emit_value(\n                expr.span,\n                Ty::Bool,\n                FirInstructionKind::Binary {\n                    op: BinaryOp::NotEq,\n                    overflow: None,\n                    left: masked,\n                    right: zero,\n                },\n            )\n        } else if result_ty == access.storage {\n            masked\n        } else {\n            self.emit_value(\n                expr.span,\n                result_ty.clone(),\n                FirInstructionKind::Convert {\n                    value: masked,\n                    target: result_ty,\n                },\n            )\n        }\n    }\n\n    fn lower_bitfield_assignment(&mut self, target: &HirExpr, value: &HirExpr) -> bool {\n        let Some(typed) = self.exprs.get(&target.id).copied() else {\n            return false;\n        };\n        let TypedExprKind::ResolvedBitField { access, .. } = &typed.kind else {\n            return false;\n        };\n        let access = access.clone();\n        let HirExprKind::Member { base, .. } = &target.kind else {\n            return false;\n        };\n        let Some(base_place) = self.lower_place(base) else {\n            return true;\n        };\n        let current = self.emit_value(\n            base.span,\n            Ty::Nominal(access.owner),\n            FirInstructionKind::Load {\n                place: base_place.clone(),\n            },\n        );\n        let storage = self.emit_value(\n            target.span,\n            access.storage.clone(),\n            FirInstructionKind::BitStructStorage {\n                value: current,\n                storage: access.storage.clone(),\n            },\n        );\n        let field_value = self.lower_expr(value);\n        if access.field.ty != Ty::Bool\n            && access.field.width < integer_width_bits(&access.field.ty).unwrap_or(access.field.width)\n        {\n            self.emit_void(\n                value.span,\n                FirInstructionKind::BitFieldCheck {\n                    value: field_value,\n                    width: access.field.width,\n                },\n            );\n        }\n        let encoded = if access.field.ty == access.storage {\n            field_value\n        } else {\n            self.emit_value(\n                value.span,\n                access.storage.clone(),\n                FirInstructionKind::Convert {\n                    value: field_value,\n                    target: access.storage.clone(),\n                },\n            )\n        };\n        let mask = bit_mask(access.field.width);\n        let mask_value = self.emit_integer_const(target.span, access.storage.clone(), mask);\n        let masked = self.emit_value(\n            target.span,\n            access.storage.clone(),\n            FirInstructionKind::Binary {\n                op: BinaryOp::BitAnd,\n                overflow: None,\n                left: encoded,\n                right: mask_value,\n            },\n        );\n        let shifted_value = if access.field.offset == 0 {\n            masked\n        } else {\n            let shift = self.emit_integer_const(\n                target.span,\n                access.storage.clone(),\n                access.field.offset as u64,\n            );\n            self.emit_value(\n                target.span,\n                access.storage.clone(),\n                FirInstructionKind::Binary {\n                    op: BinaryOp::ShiftLeft,\n                    overflow: Some(OverflowMode::Checked),\n                    left: masked,\n                    right: shift,\n                },\n            )\n        };\n        let shifted_mask = mask << access.field.offset;\n        let storage_mask = bit_mask(access.storage_bits);\n        let clear_mask = storage_mask ^ shifted_mask;\n        let clear_value =\n            self.emit_integer_const(target.span, access.storage.clone(), clear_mask);\n        let cleared = self.emit_value(\n            target.span,\n            access.storage.clone(),\n            FirInstructionKind::Binary {\n                op: BinaryOp::BitAnd,\n                overflow: None,\n                left: storage,\n                right: clear_value,\n            },\n        );\n        let combined = self.emit_value(\n            target.span,\n            access.storage.clone(),\n            FirInstructionKind::Binary {\n                op: BinaryOp::BitOr,\n                overflow: None,\n                left: cleared,\n                right: shifted_value,\n            },\n        );\n        let rebuilt = self.emit_value(\n            target.span,\n            Ty::Nominal(access.owner),\n            FirInstructionKind::BitStructFromStorage {\n                value: combined,\n                bitstruct: access.owner,\n            },\n        );\n        self.emit_void(\n            target.span,\n            FirInstructionKind::Store {\n                place: base_place,\n                value: rebuilt,\n            },\n        );\n        true\n    }\n\n    fn emit_integer_const(&mut self, span: Span, ty: Ty, value: u64) -> FirValueId {\n        self.emit_value(\n            span,\n            ty,\n            FirInstructionKind::Const {\n                value: FirConst::Integer {\n                    text: value.to_string(),\n                },\n            },\n        )\n    }\n\n    fn lower_match(&mut self, expr: &HirExpr, plan: &TypedMatchPlan, result_ty: Ty) -> FirValueId {''')

# Helpers for bit masks and standard integer field widths.
replace_once(
    fir,
    '''fn binary_overflow(op: BinaryOp, mode: OverflowMode) -> Option<OverflowMode> {''',
    '''fn bit_mask(width: u32) -> u64 {\n    if width >= 64 {\n        u64::MAX\n    } else {\n        (1u64 << width) - 1\n    }\n}\n\nfn integer_width_bits(ty: &Ty) -> Option<u32> {\n    match ty {\n        Ty::Int { width: IntWidth::W8, .. } => Some(8),\n        Ty::Int { width: IntWidth::W16, .. } => Some(16),\n        Ty::Int { width: IntWidth::W32, .. } => Some(32),\n        Ty::Int { width: IntWidth::W64, .. } => Some(64),\n        _ => None,\n    }\n}\n\nfn binary_overflow(op: BinaryOp, mode: OverflowMode) -> Option<OverflowMode> {''')

# Public exports.
replace_once(
    lib,
    '''    TypedMatchBinding, TypedMatchPlan, TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,\n};''',
    '''    TypedBitField, TypedBitFieldAccess, TypedBitStruct, TypedMatchBinding, TypedMatchPlan,\n    TypedUnsafeScope, UnsafeOperationKind, UnsafeProvenance,\n};''')

# --- tests ---
with open(ttype, "a") as f:
    f.write(r'''

#[test]
fn bitstruct_layout_is_lsb_first_with_standard_field_types() {
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
        fn main(status: Status) -> u8 { return status.mode; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let layout = output.bitstructs.values().next().expect("bitstruct layout");
    assert_eq!(layout.storage_bits, 16);
    assert_eq!(layout.fields["ready"].offset, 0);
    assert_eq!(layout.fields["error"].offset, 1);
    assert_eq!(layout.fields["mode"].offset, 2);
    assert_eq!(layout.fields["code"].offset, 5);
    assert_eq!(layout.fields["reserved"].offset, 10);
    assert_eq!(layout.fields["ready"].ty, Ty::Bool);
    assert_eq!(
        layout.fields["mode"].ty,
        Ty::Int {
            signed: false,
            width: IntWidth::W8,
        }
    );
    assert!(output
        .functions
        .values()
        .flat_map(|body| body.expressions.iter())
        .any(|expr| matches!(expr.kind, forge_frontend::TypedExprKind::ResolvedBitField { .. })));
}

#[test]
fn bitstruct_rejects_invalid_storage_and_overflowing_layout() {
    let bad_storage = check(
        r#"
        module test.bitstruct_bad_storage;
        bitstruct Bad: i16 { x: 1; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&bad_storage, "bitstruct/storage"), "{:?}", bad_storage.diagnostics);

    let too_wide = check(
        r#"
        module test.bitstruct_too_wide;
        bitstruct Bad: u8 { a: 5; b: 4; }
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(has(&too_wide, "bitstruct/width"), "{:?}", too_wide.diagnostics);
}

#[test]
fn bitstruct_constructor_requires_exact_storage_type() {
    let output = check(
        r#"
        module test.bitstruct_constructor;
        bitstruct Status: u16 { mode: 3; }
        fn main() -> i32 {
            val status: Status = Status(0u8);
            return 0;
        }
        "#,
    );
    assert!(has(&output, "bitstruct/storage-conversion"), "{:?}", output.diagnostics);
}
''')

with open(tfir, "a") as f:
    f.write(r'''

#[test]
fn bitstruct_read_lowers_to_storage_shift_and_mask() {
    let output = lower(
        r#"
        module test.fir_bitstruct_read;
        bitstruct Status: u16 { ready: 1; error: 1; mode: 3; code: 5; reserved: 6; }
        fn mode(status: Status) -> u8 { return status.mode; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::BitStructStorage { .. })));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary { op: forge_frontend::ast::BinaryOp::ShiftRight, .. }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary { op: forge_frontend::ast::BinaryOp::BitAnd, .. }
    )));
}

#[test]
fn bitstruct_write_is_checked_and_uses_read_modify_write_masks() {
    let output = lower(
        r#"
        module test.fir_bitstruct_write;
        bitstruct Status: u16 { ready: 1; error: 1; mode: 3; code: 5; reserved: 6; }
        fn update() -> u8 {
            var status: Status = Status(0u16);
            status.mode = 7u8;
            return status.mode;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::BitFieldCheck { width: 3, .. }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary { op: forge_frontend::ast::BinaryOp::ShiftLeft, .. }
    )));
    assert!(instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::Binary { op: forge_frontend::ast::BinaryOp::BitOr, .. }
    )));
    assert!(instructions(&output).any(|op| matches!(op, FirInstructionKind::BitStructFromStorage { .. })));
}

#[test]
fn one_bit_bitstruct_field_is_bool_and_needs_no_range_check() {
    let output = lower(
        r#"
        module test.fir_bitstruct_bool;
        bitstruct Flags: u8 { ready: 1; reserved: 7; }
        fn update() -> bool {
            var flags: Flags = Flags(0u8);
            flags.ready = true;
            return flags.ready;
        }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    assert!(!instructions(&output).any(|op| matches!(
        op,
        FirInstructionKind::BitFieldCheck { width: 1, .. }
    )));
}
''')

# Plan completion record and acceptance criteria.
p = Path(plan)
text = p.read_text()
text = text.replace(
    "- Step 12 complete: raw-pointer dereference/arithmetic and pointer conversions require semantic unsafe authorization; typed HIR retains exact enclosing unsafe-scope provenance and FIR carries/verifies it on dedicated raw operations.\n- Steps 13-16 intentionally untouched.",
    "- Step 12 complete: raw-pointer dereference/arithmetic and pointer conversions require semantic unsafe authorization; typed HIR retains exact enclosing unsafe-scope provenance and FIR carries/verifies it on dedicated raw operations.\n- Step 13 complete: bitstruct declarations materialize fixed unsigned storage, LSB-first offsets, standard Forge field types, checked narrow writes, and explicit FIR mask/shift read-modify-write lowering.\n- Steps 14-16 intentionally untouched.",
)
text += '''\n\n## Step 13 acceptance tests\n\n- Bitstruct storage is restricted to `u8`, `u16`, `u32`, or `u64`; zero-width and overflowing fields are semantic errors.\n- Fields are assigned monotonically from bit 0 in declaration order (LSB-first).\n- A one-bit field has ordinary Forge type `bool`; wider fields use the smallest ordinary unsigned integer type that can hold their declared width.\n- Member access carries a resolved bit-field layout in typed HIR; FIR never recomputes offsets from source declarations.\n- Reads expose storage, shift right by the resolved offset, mask to the resolved width, then convert to the ordinary field type.\n- Writes perform an explicit range check when the ordinary field type admits values wider than the field, then use mask/shift read-modify-write and rebuild the nominal bitstruct.\n- Explicit bitstruct construction requires exactly its declared storage type.\n'''
p.write_text(text)
