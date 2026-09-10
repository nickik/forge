# Forge

Forge is a statically typed, ahead-of-time systems language designed as a practical successor to C: familiar braces and operators, type-after-name declarations, immutable-by-default bindings, explicit durable allocation, flat value layouts, tagged unions, strong pattern matching, and production safety checks without a mandatory managed runtime.

This repository contains the normative Forge v1 design and the bootstrap compiler frontend.

## Status

Forge is **spec-first**. The source language is intended to remain compatible for decades, so parser or compiler implementation details must not silently become language semantics.

The active bootstrap frontend is Rust:

```text
Forge source
  -> Logos tokens + byte spans
  -> Chumsky parser
  -> source-faithful AST
  -> HIR
  -> typed HIR
  -> FIR (Forge Intermediate Representation)
  -> LLVM / Cranelift / portable C / direct native backend
```

The parser deliberately stops at a source AST. Name resolution, desugaring, type checking, representation selection, safety checking, and code generation belong to later stages.

## Parser quick start

Install a stable Rust toolchain and run:

```sh
cargo test --workspace
cargo run -p forge-frontend --bin forge-parse -- --json examples/parser-demo.fg
```

`forge-parse` emits a JSON-serializable tree with byte spans on AST nodes. This is a debugging/tooling interface, not a frozen language ABI.

## Initial parser coverage

The bring-up parser currently covers modules/imports, functions, structs, enums, tagged unions, distinct/type aliases, typed bindings, common Forge types, literals, calls/indexing, operator precedence, `if`, `while`, `return`, `defer`, and `unsafe`.

It intentionally does **not** yet implement the entire v1 grammar. Pattern matching/destructuring, closures, `@` metadata/FDN, `#tag` reader forms, named calls, and CSP syntax are staged next. See `docs/parser-status.md`.

## Repository map

- `crates/forge-frontend/` — Rust lexer, parser, AST, CLI, and parser tests.
- `docs/forge-v1-spec.md` — normative language specification.
- `docs/fdn-v1-spec.md` — Forge Data Notation specification.
- `docs/frontend-ir.md` — AST/HIR/FIR/codegen representation decisions.
- `docs/parser-status.md` — implemented parser subset and next grammar work.
- `docs/compiler-architecture.md` — compiler pipeline and optimization strategy.
- `docs/grammar.ebnf` — parser-oriented full-language grammar sketch.
- `docs/memory-model.md` — allocators, arenas, pools, scratch memory, ownership conventions.
- `docs/concurrency.md` — OS threads, CSP, agents, atomics.
- `docs/compatibility.md` — v1 compatibility promises.
- `examples/conformance/` — C-compiler-inspired run, parse, and negative Forge examples plus an FDN suite manifest.
- `AGENTS.md` — repository instructions for Codex and coding agents.
- `TASKS.md` — ordered implementation work.

## Compiler representation rule

Do **not** generate LLVM IR directly from parser nodes. Keep Forge semantics visible until semantic analysis has finished:

1. **AST** — source-faithful syntax and spans.
2. **HIR** — reader expansion, resolved names, syntax desugaring.
3. **Typed HIR** — exact types, overload resolution, closure captures, safety context.
4. **FIR** — typed function-local CFG/three-address IR with explicit checked arithmetic, checked indexing, `Option`/`Result`, calls, and cleanup edges.
5. **Backend IR** — LLVM IR, Cranelift IR, portable C, or a direct target backend.

See `docs/frontend-ir.md` for the rationale and comparable compiler designs.

## Design rule

When implementation and specification disagree, **the specification wins** unless a language change is explicitly recorded and the normative specification is updated in the same change.
