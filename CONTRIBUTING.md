# Contributing to Forge

Forge is spec-first. Compiler implementation and language design are reviewed separately.

## Workflow

1. Read `AGENTS.md` and the relevant specification section.
2. For substantial changes, write an execution plan per `PLANS.md`.
3. Implement the smallest coherent change.
4. Add positive and negative tests.
5. Run `cargo fmt --check` and `cargo test --workspace`.
6. Update documentation in the same change when semantics or developer workflow changes.
7. Review using `CODE_REVIEW.md`.

## Language changes

A language change must include:

- motivation;
- exact syntax;
- static semantics;
- runtime semantics;
- lowering expectations where relevant;
- compatibility analysis;
- examples and counterexamples;
- parser/type-checker tests.

Do not treat implementation convenience as language rationale.

## Rust frontend

- Format with `cargo fmt`.
- Prefer explicit AST/HIR types over unstructured maps.
- Preserve byte spans through parser and semantic lowering.
- Keep semantic analysis out of the parser.
- Do not introduce LLVM/Cranelift types into AST or HIR.
- Avoid new dependencies unless they clearly reduce implementation risk.

Markdown should remain readable as plain text.
