include!("typecheck_v1.rs");

/// Code-generation-facing structural description of a resolved Forge type.
///
/// This deliberately records source declaration indices even though Forge's
/// default memory layout may reorder fields.  The layout engine owns the
/// physical ordering; the type checker owns field identity and resolved types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeFieldDefinition {
    pub name: String,
    pub ty: Ty,
    pub declaration_index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeVariantDefinition {
    pub name: String,
    pub declaration_index: u32,
    pub fields: Vec<TypeFieldDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TypeDefinitionKind {
    Alias {
        target: Ty,
    },
    Distinct {
        underlying: Ty,
    },
    Struct {
        fields: Vec<TypeFieldDefinition>,
    },
    Enum {
        variants: Vec<TypeVariantDefinition>,
    },
    Tagged {
        variants: Vec<TypeVariantDefinition>,
    },
    BitStruct {
        storage: Ty,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeDefinition {
    pub owner: DefId,
    pub kind: TypeDefinitionKind,
}

pub type TypeDefinitionTable = BTreeMap<DefId, TypeDefinition>;

/// Recover the fully-resolved structural type table used by FIR layout/codegen.
///
/// `TypeCheckOutput` intentionally remains focused on semantic checking.  C9
/// needs one additional code-generation side table containing nominal field and
/// variant structure.  Build it with the same `ModuleTypeEnv` used by the type
/// checker, rather than re-resolving names in a backend.
///
/// For a valid typed module the scratch environment is equivalent to the one
/// used by `type_check_module`.  Any diagnostics produced while rebuilding it
/// were already produced by type checking and are therefore not duplicated.
pub fn collect_type_definitions(
    source: &ast::SourceFile,
    module: &HirModule,
    bodies: &BodyHirOutput,
    typed: &TypeCheckOutput,
) -> TypeDefinitionTable {
    let mut ignored_diagnostics = Vec::new();
    let env = ModuleTypeEnv::build(
        source,
        module,
        &typed.constants,
        bodies,
        &mut ignored_diagnostics,
    );

    let mut definitions = BTreeMap::new();
    for (index, declaration) in source.declarations.iter().enumerate() {
        let owner = DefId(index as u32);
        let kind = match &declaration.kind.kind {
            DeclKind::Distinct(_) => match env.types.get(&owner).map(|info| &info.kind) {
                Some(TypeInfoKind::Distinct(underlying)) => TypeDefinitionKind::Distinct {
                    underlying: underlying.clone(),
                },
                _ => continue,
            },
            DeclKind::TypeAlias(_) => match env.types.get(&owner).map(|info| &info.kind) {
                Some(TypeInfoKind::Alias(target)) => TypeDefinitionKind::Alias {
                    target: target.clone(),
                },
                _ => continue,
            },
            DeclKind::Struct(value) => TypeDefinitionKind::Struct {
                fields: value
                    .fields
                    .iter()
                    .enumerate()
                    .map(|(field_index, field)| TypeFieldDefinition {
                        name: field.name.clone(),
                        ty: env.lower_ast_type(&field.ty, module),
                        declaration_index: field_index as u32,
                    })
                    .collect(),
            },
            DeclKind::Enum(value) => TypeDefinitionKind::Enum {
                variants: value
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(variant_index, variant)| TypeVariantDefinition {
                        name: variant.name.clone(),
                        declaration_index: variant_index as u32,
                        fields: Vec::new(),
                    })
                    .collect(),
            },
            DeclKind::Tagged(value) => TypeDefinitionKind::Tagged {
                variants: value
                    .variants
                    .iter()
                    .enumerate()
                    .map(|(variant_index, variant)| TypeVariantDefinition {
                        name: variant.name.clone(),
                        declaration_index: variant_index as u32,
                        fields: variant
                            .fields
                            .iter()
                            .enumerate()
                            .map(|(field_index, field)| TypeFieldDefinition {
                                name: field.name.clone(),
                                ty: env.lower_ast_type(&field.ty, module),
                                declaration_index: field_index as u32,
                            })
                            .collect(),
                    })
                    .collect(),
            },
            DeclKind::BitStruct(_) => match env.types.get(&owner).map(|info| &info.kind) {
                Some(TypeInfoKind::BitStruct(layout)) => TypeDefinitionKind::BitStruct {
                    storage: layout.storage.clone(),
                },
                _ => continue,
            },
            _ => continue,
        };
        definitions.insert(owner, TypeDefinition { owner, kind });
    }
    definitions
}
