#!/usr/bin/env python3
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]


def read(path: str) -> str:
    return (ROOT / path).read_text()


def write(path: str, text: str) -> None:
    (ROOT / path).write_text(text)


def replace_once(path: str, old: str, new: str) -> None:
    text = read(path)
    count = text.count(old)
    if count != 1:
        raise RuntimeError(f"{path}: expected one occurrence, found {count}: {old[:80]!r}")
    write(path, text.replace(old, new, 1))


def append_once(path: str, marker: str, addition: str) -> None:
    text = read(path)
    if marker in text:
        return
    write(path, text.rstrip() + "\n\n" + addition.strip() + "\n")


# ---------------------------------------------------------------------------
# AST: metadata is no longer an expression/type wrapper.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/ast_v1.rs",
    """    Annotated {\n        value: Box<Expr>,\n        metadata: Vec<Metadata>,\n    },\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/ast_v1.rs",
    """    Annotated {\n        inner: Box<TypeNode>,\n        metadata: Vec<Metadata>,\n    },\n""",
    "",
)

# ---------------------------------------------------------------------------
# Parser: @ metadata is prefix-only on declarations/declaration-owned fields.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """    enum TypeSuffix {\n        Optional,\n        Slice(bool),\n        Metadata(Metadata),\n    }\n""",
    """    enum TypeSuffix {\n        Optional,\n        Slice(bool),\n    }\n""",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """        just(Token::LBracket)\n            .ignore_then(just(Token::RBracket))\n            .ignore_then(just(Token::Mut).or_not())\n            .map(|mutable| TypeSuffix::Slice(mutable.is_some())),\n        metadata.clone().map(TypeSuffix::Metadata),\n""",
    """        just(Token::LBracket)\n            .ignore_then(just(Token::RBracket))\n            .ignore_then(just(Token::Mut).or_not())\n            .map(|mutable| TypeSuffix::Slice(mutable.is_some())),\n""",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """                    TypeSuffix::Metadata(item) => TypeKind::Annotated {\n                        inner: Box::new(base),\n                        metadata: vec![item],\n                    },\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """    let wrap_expr = just(Token::At)\n        .ignore_then(select! { Token::Ident(name) if name == \"wrap\" => name })\n        .then(\n            expr.clone()\n                .delimited_by(just(Token::LParen), just(Token::RParen)),\n        )\n        .map_with(|(name, value), e| {\n            Node::new(\n                ExprKind::Annotated {\n                    value: Box::new(value),\n                    metadata: vec![Metadata {\n                        name: Some(name),\n                        arguments: Vec::new(),\n                        map: None,\n                    }],\n                },\n                span(e.span()),\n            )\n        });\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """        match_expr,\n        wrap_expr,\n        reader_expr,\n""",
    """        match_expr,\n        reader_expr,\n""",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """        Member(String),\n        Try,\n        Metadata(Metadata),\n""",
    """        Member(String),\n        Try,\n""",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """        just(Token::Dot).ignore_then(ident()).map(Postfix::Member),\n        just(Token::Question).to(Postfix::Try),\n        metadata.clone().map(Postfix::Metadata),\n""",
    """        just(Token::Dot).ignore_then(ident()).map(Postfix::Member),\n        just(Token::Question).to(Postfix::Try),\n""",
)
replace_once(
    "crates/forge-frontend/src/parser_v1.rs",
    """            Postfix::Metadata(item) => ExprKind::Annotated {\n                value: Box::new(base),\n                metadata: vec![item],\n            },\n""",
    "",
)

# ---------------------------------------------------------------------------
# Body HIR: remove expression/type annotation wrapper nodes.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    """    Annotated {\n        value: Box<HirExpr>,\n        metadata: Vec<ast::Metadata>,\n    },\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    """    Annotated {\n        inner: Box<HirType>,\n        metadata: Vec<ast::Metadata>,\n    },\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    """            TypeKind::Annotated { inner, metadata } => HirTypeKind::Annotated {\n                inner: Box::new(self.lower_type(inner)),\n                metadata: metadata.clone(),\n            },\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/body_hir_v1.rs",
    """            ExprKind::Annotated { value, metadata } => HirExprKind::Annotated {\n                value: Box::new(self.lower_expr(value)),\n                metadata: metadata.clone(),\n            },\n""",
    "",
)

# ---------------------------------------------------------------------------
# HIR: one universal metadata table, keyed by semantic target.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """pub struct DefId(pub u32);\n\n""",
    """pub struct DefId(pub u32);\n\n#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]\n#[serde(tag = \"target\", rename_all = \"snake_case\")]\npub enum MetadataTarget {\n    Item {\n        owner: DefId,\n    },\n    Field {\n        owner: DefId,\n        variant: Option<String>,\n        name: String,\n    },\n    ImplMethod {\n        owner: DefId,\n        name: String,\n    },\n}\n\npub type MetadataTable = BTreeMap<MetadataTarget, Vec<ast::Metadata>>;\n\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """    pub items: Vec<HirItem>,\n    pub symbols: BTreeMap<String, SymbolSet>,\n""",
    """    pub items: Vec<HirItem>,\n    pub symbols: BTreeMap<String, SymbolSet>,\n    pub metadata: MetadataTable,\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """        items: Vec::with_capacity(source.declarations.len()),\n        symbols: BTreeMap::new(),\n""",
    """        items: Vec::with_capacity(source.declarations.len()),\n        symbols: BTreeMap::new(),\n        metadata: BTreeMap::new(),\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """    for (index, declaration) in source.declarations.iter().enumerate() {\n        let id = DefId(index as u32);\n        let kind = match &declaration.kind.kind {\n""",
    """    for (index, declaration) in source.declarations.iter().enumerate() {\n        let id = DefId(index as u32);\n        record_metadata(\n            &mut module.metadata,\n            MetadataTarget::Item { owner: id },\n            &declaration.kind.metadata,\n        );\n        let kind = match &declaration.kind.kind {\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """            DeclKind::Struct(value) => {\n                define_type(\n""",
    """            DeclKind::Struct(value) => {\n                for field in &value.fields {\n                    record_metadata(\n                        &mut module.metadata,\n                        MetadataTarget::Field {\n                            owner: id,\n                            variant: None,\n                            name: field.name.clone(),\n                        },\n                        &field.metadata,\n                    );\n                }\n                define_type(\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """            DeclKind::Tagged(value) => {\n                define_type(\n""",
    """            DeclKind::Tagged(value) => {\n                for variant in &value.variants {\n                    for field in &variant.fields {\n                        record_metadata(\n                            &mut module.metadata,\n                            MetadataTarget::Field {\n                                owner: id,\n                                variant: Some(variant.name.clone()),\n                                name: field.name.clone(),\n                            },\n                            &field.metadata,\n                        );\n                    }\n                }\n                define_type(\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """            DeclKind::Impl(value) => HirItemKind::Impl {\n                target: value.target.clone(),\n            },\n""",
    """            DeclKind::Impl(value) => {\n                for method in &value.methods {\n                    record_metadata(\n                        &mut module.metadata,\n                        MetadataTarget::ImplMethod {\n                            owner: id,\n                            name: method.function.name.clone(),\n                        },\n                        &method.metadata,\n                    );\n                }\n                HirItemKind::Impl {\n                    target: value.target.clone(),\n                }\n            }\n""",
)
replace_once(
    "crates/forge-frontend/src/hir_v1.rs",
    """fn define_type(\n""",
    """fn record_metadata(\n    table: &mut MetadataTable,\n    target: MetadataTarget,\n    metadata: &[ast::Metadata],\n) {\n    if !metadata.is_empty() {\n        table.entry(target).or_default().extend_from_slice(metadata);\n    }\n}\n\nfn define_type(\n""",
)

# ---------------------------------------------------------------------------
# Typed HIR output: preserve the same generic metadata table downstream.
# ---------------------------------------------------------------------------
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """    hir::{DefId, HirModule},\n""",
    """    hir::{DefId, HirModule, MetadataTable},\n""",
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """pub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub global_types: BTreeMap<DefId, Ty>,\n    pub diagnostics: Vec<TypeDiagnostic>,\n}\n""",
    """pub struct TypeCheckOutput {\n    pub functions: BTreeMap<DefId, TypedBody>,\n    pub global_types: BTreeMap<DefId, Ty>,\n    pub metadata: MetadataTable,\n    pub diagnostics: Vec<TypeDiagnostic>,\n}\n""",
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """    let mut output = TypeCheckOutput::default();\n    let env = ModuleTypeEnv::build(source, module);\n""",
    """    let mut output = TypeCheckOutput {\n        metadata: module.metadata.clone(),\n        ..TypeCheckOutput::default()\n    };\n    let env = ModuleTypeEnv::build(source, module);\n""",
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """            ast::TypeKind::Annotated { inner, .. } => self.lower_ast_type(inner, module),\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """            HirTypeKind::Annotated { inner, .. } => self.lower_hir_type(inner),\n""",
    "",
)
replace_once(
    "crates/forge-frontend/src/typecheck_v1.rs",
    """            HirExprKind::Annotated { value, .. } => self.check_expr(value, expected),\n""",
    "",
)

# Re-export the universal metadata model.
replace_once(
    "crates/forge-frontend/src/lib.rs",
    """pub use hir::{lower_module, DefId, HirDiagnostic, HirModule, HirOutput, Namespace};\n""",
    """pub use hir::{\n    lower_module, DefId, HirDiagnostic, HirModule, HirOutput, MetadataTable, MetadataTarget,\n    Namespace,\n};\n""",
)

# ---------------------------------------------------------------------------
# Grammar/specification cleanup.
# ---------------------------------------------------------------------------
grammar = read("docs/grammar.ebnf")
grammar = grammar.replace(
    'metadata         = "@", identifier, [ "(", fdn_argument_list, ")" ]\n                 | "@", fdn_map ;\npostfix_metadata = "@", identifier, [ "(", fdn_argument_list, ")" ] ;\n',
    'metadata         = "@", identifier, [ "(", fdn_argument_list, ")" ]\n                 | "@", fdn_map ;\n',
)
grammar = grammar.replace(
    'type_decl       = "type", identifier, "=", type, { postfix_metadata }, ";" ;',
    'type_decl       = "type", identifier, "=", type, ";" ;',
)
grammar = grammar.replace(
    'type_suffix     = "?" | "[", "]", [ "mut" ] | postfix_metadata ;',
    'type_suffix     = "?" | "[", "]", [ "mut" ] ;',
)
grammar = grammar.replace(
    'expression      = match_expression | logical_or_expression, { postfix_metadata } ;',
    'expression      = match_expression | logical_or_expression ;',
)
if "postfix_metadata" in grammar:
    raise RuntimeError("grammar.ebnf still contains postfix_metadata")
write("docs/grammar.ebnf", grammar)

spec = read("docs/forge-v1-spec.md")
spec = spec.replace(
    """```forge\ntype Percentage = u8 @range(0..=100);\n```""",
    """```forge\n@range(0..=100)\ntype Percentage = u8;\n```""",
)
spec = spec.replace(
    """Explicit wrapping:\n\n```forge\nval c: u32 = @wrap(a + b);\n```\n\nFunction/block metadata may declare wrapping arithmetic where appropriate:\n\n```forge\n@overflow(wrap)\nfn hash_mix(x: u32) -> u32 {\n    return x * 2654435761u32;\n}\n```\n""",
    """Wrapping arithmetic is not expressed by attaching metadata to an expression. When an entire function intentionally uses wrapping integer arithmetic, declaration metadata may request it:\n\n```forge\n@overflow(wrap)\nfn hash_mix(x: u32) -> u32 {\n    return x * 2654435761u32;\n}\n```\n\nCode that needs only one localized wrapping operation uses an explicit wrapping arithmetic operation/intrinsic rather than `@` expression syntax.\n""",
)
spec = spec.replace(
    """Unchecked indexing requires explicit unsafe intent:\n\n```forge\nunsafe {\n    val x = values[i] @unchecked;\n}\n```\n\nOptimization level alone never changes checked source semantics.\n""",
    """Unchecked indexing requires explicit unsafe intent and a dedicated unsafe operation/intrinsic. `@` metadata is not expression syntax, and entering `unsafe` alone does not disable bounds checks.\n\nOptimization level alone never changes checked source semantics.\n""",
)
spec = spec.replace(
    """Metadata can be consumed by compiler, linker, documentation, serialization and static-analysis tools. Unknown metadata must not silently change core language semantics.\n""",
    """Metadata attaches to declarations and declaration-owned fields/methods. It is not an expression operator or a postfix type operator: forms such as `value @unchecked`, `u8 @range(...)`, and `@wrap(expr)` are not Forge v1 metadata syntax. A constrained alias instead carries `@range(...)` on the alias declaration itself.\n\nThe compiler carries metadata as one universal structured metadata concept. HIR and later semantic stages retain the original metadata values together with the semantic target they annotate. Individual compiler stages interpret only metadata names they own (`@repr`, `@align`, `@overflow`, and so on); they must not lower each metadata spelling into unrelated parser/HIR syntax. Unknown metadata remains available to linker, documentation, serialization and static-analysis tools and must not silently change core language semantics.\n""",
)
write("docs/forge-v1-spec.md", spec)

syntax = read("docs/forge-v1-syntax-decisions.md")nstart = syntax.index("## Metadata\n")
end = syntax.index("\n## Methods\n", start)
metadata_section = """## Metadata\n\nMetadata is prefix-only in Forge v1. It may annotate declarations and declaration-owned fields/methods.\n\n```forge\n@repr(c)\nstruct Header {\n    @deprecated(\"use sequence\")\n    sequence: u32;\n}\n\n@range(0..=100)\ntype Percentage = u8;\n\n@overflow(wrap)\nfn hash_mix(x: u32) -> u32 {\n    return x * 2654435761u32;\n}\n```\n\nMetadata is structured data attached to a semantic target. The compiler preserves it generically through HIR/typed HIR; later stages inspect names they understand instead of receiving separate syntax nodes for each metadata spelling.\n\nPostfix type/expression metadata is not part of v1. These forms are rejected:\n\n```forge\ntype Percentage = u8 @range(0..=100);\nval x = values[i] @unchecked;\nval y = @wrap(a + b);\n```\n\nChecked arithmetic and checked indexing remain the default. Function-wide wrapping may be requested with `@overflow(wrap)`. Localized wrapping or unchecked operations use explicit operations/intrinsics, not metadata applied to expressions. `@check(...)` is likewise not a standardized compiler form.\n"""
syntax = syntax[:start] + metadata_section + syntax[end:]
write("docs/forge-v1-syntax-decisions.md", syntax)

arch = read("docs/compiler-architecture.md")
needle = "A second typed-HIR stage records exact Forge types, resolved overloads, closure captures and safety checks before FIR lowering.\n"
replacement = needle + "\nMetadata is lowered separately from expression/type syntax into one generic target-keyed metadata table. HIR assigns metadata to semantic targets (items, fields, impl methods, and future declaration-owned targets), and typed HIR carries the same table forward. Layout, optimizer, linker and tooling passes interpret the metadata names relevant to them; the frontend does not create one bespoke HIR field or node kind per metadata spelling.\n"
if needle not in arch:
    raise RuntimeError("compiler-architecture HIR paragraph not found")
arch = arch.replace(needle, replacement, 1)
write("docs/compiler-architecture.md", arch)

frontend_ir = read("docs/frontend-ir.md")
needle = "- retain source spans for diagnostics.\n"
replacement = "- retain source spans for diagnostics;\n- retain one generic metadata table keyed by semantic target so later stages can inspect declaration metadata without reparsing AST syntax.\n"
if needle not in frontend_ir:
    raise RuntimeError("frontend-ir HIR requirement marker not found")
frontend_ir = frontend_ir.replace(needle, replacement, 1)
write("docs/frontend-ir.md", frontend_ir)

parser_status = read("docs/parser-status.md")
parser_status = parser_status.replace(
    "- JSON AST dumping\n",
    "- JSON AST dumping\n- prefix metadata on declarations and declaration-owned fields/methods\n- structured FDN metadata payloads\n",
)
parser_status = parser_status.replace("7. `@` metadata carrying FDN values;\n", "")
write("docs/parser-status.md", parser_status)

# ---------------------------------------------------------------------------
# Positive conformance examples use prefix metadata only.
# ---------------------------------------------------------------------------
write(
    "examples/conformance/parse/10-metadata-readers.fg",
    '''module examples.conformance.metadata_readers;\n\n@{\n    :since #version "1.0.0"\n    :doc/category :network\n}\npub struct Connection {\n    @deprecated("legacy")\n    id: u32;\n}\n\n@inline\n@overflow(wrap)\nfn metadata(value: u32) -> u32 {\n    val id = #uuid "550e8400-e29b-41d4-a716-446655440000";\n    val instant = #inst "1985-04-12T23:20:50.52Z";\n    val table = #forge/array {\n        :type #type "u32"\n        :size 4\n        :init 0\n    };\n    return value + 1u32;\n}\n''',
)
write(
    "examples/conformance/parse/22-metadata-forms.fg",
    '''module examples.conformance.metadata_forms;\n\n@inline()\n@overflow(checked)\n@{\n    :owner "compiler"\n    :since #version "1.0"\n}\nfn checked(value: u32) -> u32 {\n    return value + 1u32;\n}\n\n@range(0..=100)\ntype Percent = u8;\n''',
)
write(
    "examples/conformance/parse/33-metadata-chains.fg",
    '''module examples.conformance.metadata_chains;\n\n@cold\n@doc("helper")\nfn helper(value: u32) -> u32 {\n    return value;\n}\n\n@range(0..=15)\n@repr(u8)\ntype Small = u32;\n''',
)

# Explicit rejection cases for the removed expression/type forms.
write(
    "examples/conformance/syntax-negative/65-postfix-expression-metadata.fg",
    '''module examples.conformance.bad_postfix_expression_metadata;\n\nfn main() -> i32 {\n    val x = 1u32 @unchecked;\n    return 0;\n}\n''',
)
write(
    "examples/conformance/syntax-negative/66-postfix-type-metadata.fg",
    '''module examples.conformance.bad_postfix_type_metadata;\n\ntype Percent = u8 @range(0..=100);\n''',
)
write(
    "examples/conformance/syntax-negative/67-wrap-expression.fg",
    '''module examples.conformance.bad_wrap_expression;\n\nfn main() -> u32 {\n    return @wrap(1u32 + 2u32);\n}\n''',
)

suite = read("examples/conformance/suite.fdn")
marker = '    {:path #path "syntax-negative/64-select-empty-arm-syntax.fg" :kind :syntax-negative :expect :syntax/select-arm}\n'
addition = marker + '    {:path #path "syntax-negative/65-postfix-expression-metadata.fg" :kind :syntax-negative :expect :syntax/metadata-placement}\n    {:path #path "syntax-negative/66-postfix-type-metadata.fg" :kind :syntax-negative :expect :syntax/metadata-placement}\n    {:path #path "syntax-negative/67-wrap-expression.fg" :kind :syntax-negative :expect :syntax/wrap-expression-removed}\n'
if marker not in suite:
    raise RuntimeError("suite syntax-negative marker not found")
suite = suite.replace(marker, addition, 1)
write("examples/conformance/suite.fdn", suite)

# ---------------------------------------------------------------------------
# Tests: parser rejection + HIR and typed-HIR metadata propagation.
# ---------------------------------------------------------------------------
append_once(
    "crates/forge-frontend/tests/parser.rs",
    "fn metadata_is_prefix_only()",
    r'''
#[test]
fn metadata_is_prefix_only() {
    let parsed = parse_source(
        r#"
        module test.metadata_prefix;

        @repr(c)
        struct Header {
            @deprecated("legacy")
            word: u32;
        }

        @range(0..=100)
        type Percent = u8;

        @overflow(wrap)
        fn mix(x: u32) -> u32 { return x + 1u32; }
        "#,
    );
    assert!(parsed.diagnostics.is_empty(), "{:?}", parsed.diagnostics);
    assert!(parsed.ast.is_some());
}

#[test]
fn rejects_expression_and_type_metadata_forms() {
    for source in [
        "module test.bad_expr_meta; fn main() -> i32 { val x = 1u32 @unchecked; return 0; }",
        "module test.bad_type_meta; type Percent = u8 @range(0..=100);",
        "module test.bad_wrap; fn main() -> u32 { return @wrap(1u32 + 2u32); }",
    ] {
        let parsed = parse_source(source);
        assert!(
            parsed.ast.is_none() || !parsed.diagnostics.is_empty(),
            "unexpectedly accepted: {source}"
        );
    }
}
''',
)

hir_tests = read("crates/forge-frontend/tests/hir.rs")
hir_tests = hir_tests.replace(
    "hir::{DefId, HirItemKind},",
    "hir::{DefId, HirItemKind, MetadataTarget},",
)
write("crates/forge-frontend/tests/hir.rs", hir_tests)
append_once(
    "crates/forge-frontend/tests/hir.rs",
    "fn preserves_metadata_by_semantic_target()",
    r'''
#[test]
fn preserves_metadata_by_semantic_target() {
    let output = lower(
        r#"
        module test.metadata_hir;

        @repr(c)
        struct Header {
            @align(4)
            word: u32;
        }

        @overflow(wrap)
        fn hash(x: u32) -> u32 { return x + 1u32; }

        impl Header {
            @inline
            fn get(self: &Header) -> u32 { return self.word; }
        }
        "#,
    );

    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let item = output
        .module
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(0) })
        .expect("struct metadata");
    assert!(item.iter().any(|m| m.name.as_deref() == Some("repr")));

    let field = output
        .module
        .metadata
        .get(&MetadataTarget::Field {
            owner: DefId(0),
            variant: None,
            name: "word".into(),
        })
        .expect("field metadata");
    assert!(field.iter().any(|m| m.name.as_deref() == Some("align")));

    let function = output
        .module
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(1) })
        .expect("function metadata");
    assert!(function
        .iter()
        .any(|m| m.name.as_deref() == Some("overflow")));

    let method = output
        .module
        .metadata
        .get(&MetadataTarget::ImplMethod {
            owner: DefId(2),
            name: "get".into(),
        })
        .expect("impl method metadata");
    assert!(method.iter().any(|m| m.name.as_deref() == Some("inline")));
}
''',
)

# Root re-exports make this test assert propagation all the way through type checking.
type_tests = read("crates/forge-frontend/tests/typecheck.rs")
type_tests = type_tests.replace(
    "lower_module, lower_resolved_bodies, parse_source, type_check_module, IntWidth, Ty,",
    "lower_module, lower_resolved_bodies, parse_source, type_check_module, DefId, IntWidth, MetadataTarget, Ty,",
)
write("crates/forge-frontend/tests/typecheck.rs", type_tests)
append_once(
    "crates/forge-frontend/tests/typecheck.rs",
    "fn typed_hir_preserves_generic_metadata_table()",
    r'''
#[test]
fn typed_hir_preserves_generic_metadata_table() {
    let output = check(
        r#"
        module test.metadata_typed;
        @inline
        @overflow(wrap)
        fn main() -> i32 { return 0; }
        "#,
    );
    assert!(output.diagnostics.is_empty(), "{:?}", output.diagnostics);
    let metadata = output
        .metadata
        .get(&MetadataTarget::Item { owner: DefId(0) })
        .expect("typed metadata");
    assert!(metadata.iter().any(|m| m.name.as_deref() == Some("inline")));
    assert!(metadata
        .iter()
        .any(|m| m.name.as_deref() == Some("overflow")));
}
''',
)

# ---------------------------------------------------------------------------
# Sanity assertions before rustfmt/build.
# ---------------------------------------------------------------------------
for path in [
    "crates/forge-frontend/src/ast_v1.rs",
    "crates/forge-frontend/src/parser_v1.rs",
    "crates/forge-frontend/src/body_hir_v1.rs",
    "crates/forge-frontend/src/typecheck_v1.rs",
]:
    text = read(path)
    if "Annotated" in text:
        raise RuntimeError(f"{path}: obsolete Annotated metadata wrapper remains")

for path in [
    "examples/conformance/parse/10-metadata-readers.fg",
    "examples/conformance/parse/22-metadata-forms.fg",
    "examples/conformance/parse/33-metadata-chains.fg",
]:
    text = read(path)
    for forbidden in ("@unchecked", "@wrap(", " @range("):
        if forbidden in text:
            raise RuntimeError(f"{path}: obsolete expression/type metadata form remains: {forbidden}")

print("metadata simplification patch applied")
