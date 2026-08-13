# Forge Bootstrap Implementation Roadmap

## M0 — Repository and specification

- Build scaffold.
- Normative Forge and FDN documents.
- Diagnostic conventions.
- Test harness conventions.

Exit: repository builds and tests; language decisions are indexed.

## M1 — Lexer + source manager

- UTF-8 source bytes with source spans.
- Identifiers/keywords.
- integers/floats/chars/strings.
- punctuation/operators.
- comments.
- `#tag` and `@` tokens.

Exit: token golden tests and malformed-literal diagnostics.

## M2 — FDN reader

- scalars, keywords/symbols;
- vectors/lists/maps/sets;
- comments/discard;
- tagged values;
- built-in UUID/instant/duration/size/bytes/path/version types;
- preserving mode for unknown tags.

Exit: round-trip and canonicalization tests.

## M3 — Parser

- modules/imports;
- `val`/`var`;
- functions and `nfn`;
- structs/enums/tagged unions/distinct aliases;
- control flow;
- Pratt expressions;
- type syntax;
- lambdas;
- basic patterns and `match`;
- metadata attachment.

Exit: parser accepts language-tour examples and rejects malformed cases deterministically.

## M4 — Semantic model

- symbol tables/modules;
- type interning;
- strict conversion checking;
- exact overload resolution;
- definite assignment;
- mutation rules;
- `Option` and `Result` constructors;
- unsafe-context checking.

Exit: type-check-only compiler mode passes semantic suite.

## M5 — Pattern compiler

- exhaustiveness;
- unreachable arms;
- nested variants/structs;
- ranges/or/and/as patterns;
- sequence and map-key patterns;
- guards;
- irrefutable destructuring validation.

Exit: generated decision trees visible in diagnostic dump mode.

## M6 — Typed IR + portable C backend

- CFG and three-address IR;
- checked arithmetic/indexing operations;
- calls/function pointers/closures;
- defer lowering;
- option/result lowering;
- emit defensive C preserving Forge semantics.

Exit: compile/run core semantic tests through host C compiler.

## M7 — Optimization baseline

- constant folding;
- LVN;
- copy propagation;
- DCE;
- liveness;
- bounds-check elimination/hoisting;
- linear-scan register allocator framework.

## M8 — Standard memory library

- allocator interface;
- linear/static/pool/slab/free-list arenas;
- unmanaged concrete vectors;
- managed wrappers;
- scratch/context infrastructure;
- allocation-failure tests.

## M9 — Threads/CSP/agents

- OS thread wrapper;
- atomics/synchronization;
- typed channel generation;
- select lowering/library interface;
- agent/action queue model.

## M10 — Native backend

Implement one native backend end-to-end. Add instruction selection, register allocation and peepholes. Differential-test against C backend.

## Later, not v1 blockers

- SSA optimizer;
- user-defined generics;
- heap-escaping closures;
- advanced compile-time evaluation;
- additional native backends;
- self-hosting compiler.
