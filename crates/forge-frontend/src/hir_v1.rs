use std::collections::BTreeMap;

use serde::Serialize;

use crate::ast::{self, DeclKind, PatternKind, Span};

/// Stable within one lowered module. IDs are assigned in source declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct DefId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Namespace {
    Type,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DefKind {
    Function,
    Struct,
    Enum,
    Tagged,
    BitStruct,
    Distinct,
    TypeAlias,
    Impl,
    Global,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct SymbolSet {
    pub type_def: Option<DefId>,
    pub value_def: Option<DefId>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirModule {
    pub module: ast::Path,
    pub imports: Vec<ast::Path>,
    pub items: Vec<HirItem>,
    pub symbols: BTreeMap<String, SymbolSet>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirItem {
    pub id: DefId,
    pub span: Span,
    pub public: bool,
    pub kind: HirItemKind,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum HirItemKind {
    Function { name: String, named_arguments: bool },
    Struct { name: String },
    Enum { name: String },
    Tagged { name: String },
    BitStruct { name: String },
    Distinct { name: String },
    TypeAlias { name: String },
    Impl { target: ast::Path },
    Global { bindings: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HirDiagnostic {
    pub span: Span,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HirOutput {
    pub module: HirModule,
    pub diagnostics: Vec<HirDiagnostic>,
}

pub fn lower_module(source: &ast::SourceFile) -> HirOutput {
    let mut module = HirModule {
        module: source.module.clone(),
        imports: source.imports.clone(),
        items: Vec::with_capacity(source.declarations.len()),
        symbols: BTreeMap::new(),
    };
    let mut diagnostics = Vec::new();

    for (index, declaration) in source.declarations.iter().enumerate() {
        let id = DefId(index as u32);
        let kind = match &declaration.kind.kind {
            DeclKind::Function(function) => {
                define(
                    &mut module.symbols,
                    &mut diagnostics,
                    &function.name,
                    Namespace::Value,
                    id,
                    declaration.span,
                );
                HirItemKind::Function {
                    name: function.name.clone(),
                    named_arguments: function.named_arguments,
                }
            }
            DeclKind::Struct(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::Struct {
                    name: value.name.clone(),
                }
            }
            DeclKind::Enum(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::Enum {
                    name: value.name.clone(),
                }
            }
            DeclKind::Tagged(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::Tagged {
                    name: value.name.clone(),
                }
            }
            DeclKind::BitStruct(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::BitStruct {
                    name: value.name.clone(),
                }
            }
            DeclKind::Distinct(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::Distinct {
                    name: value.name.clone(),
                }
            }
            DeclKind::TypeAlias(value) => {
                define_type(&mut module, &mut diagnostics, &value.name, id, declaration.span);
                HirItemKind::TypeAlias {
                    name: value.name.clone(),
                }
            }
            DeclKind::Impl(value) => HirItemKind::Impl {
                target: value.target.clone(),
            },
            DeclKind::Global(value) => {
                let mut bindings = Vec::new();
                collect_pattern_bindings(&value.pattern, &mut bindings);
                for name in &bindings {
                    define(
                        &mut module.symbols,
                        &mut diagnostics,
                        name,
                        Namespace::Value,
                        id,
                        declaration.span,
                    );
                }
                HirItemKind::Global { bindings }
            }
        };

        module.items.push(HirItem {
            id,
            span: declaration.span,
            public: declaration.kind.public,
            kind,
        });
    }

    HirOutput {
        module,
        diagnostics,
    }
}

fn define_type(
    module: &mut HirModule,
    diagnostics: &mut Vec<HirDiagnostic>,
    name: &str,
    id: DefId,
    span: Span,
) {
    define(
        &mut module.symbols,
        diagnostics,
        name,
        Namespace::Type,
        id,
        span,
    );
}

fn define(
    symbols: &mut BTreeMap<String, SymbolSet>,
    diagnostics: &mut Vec<HirDiagnostic>,
    name: &str,
    namespace: Namespace,
    id: DefId,
    span: Span,
) {
    let entry = symbols.entry(name.to_owned()).or_default();
    let slot = match namespace {
        Namespace::Type => &mut entry.type_def,
        Namespace::Value => &mut entry.value_def,
    };

    if let Some(previous) = *slot {
        diagnostics.push(HirDiagnostic {
            span,
            message: format!(
                "duplicate {namespace:?} definition `{name}`; previous definition is {:?}",
                previous
            ),
        });
    } else {
        *slot = Some(id);
    }
}

fn collect_pattern_bindings(pattern: &ast::Pattern, output: &mut Vec<String>) {
    match &pattern.kind {
        PatternKind::Binding { name, .. } => output.push(name.clone()),
        PatternKind::Variant { fields, .. } | PatternKind::Struct { fields, .. } => {
            for field in fields {
                if let Some(pattern) = &field.pattern {
                    collect_pattern_bindings(pattern, output);
                } else {
                    output.push(field.name.clone());
                }
            }
        }
        PatternKind::Sequence { items, rest } => {
            for item in items {
                collect_pattern_bindings(item, output);
            }
            if let Some(rest) = rest {
                output.push(rest.clone());
            }
        }
        PatternKind::Map { entries, .. } => {
            for entry in entries {
                output.push(entry.binding.clone());
            }
        }
        PatternKind::Some { value } | PatternKind::As { pattern: value, .. } => {
            collect_pattern_bindings(value, output);
            if let PatternKind::As { name, .. } = &pattern.kind {
                output.push(name.clone());
            }
        }
        PatternKind::Or { patterns } => {
            // Binding compatibility between alternatives is a later semantic check.
            // For item collection, use the first branch as the declaration shape.
            if let Some(first) = patterns.first() {
                collect_pattern_bindings(first, output);
            }
        }
        PatternKind::Wildcard
        | PatternKind::Literal { .. }
        | PatternKind::Range { .. }
        | PatternKind::None { .. } => {}
    }
}
