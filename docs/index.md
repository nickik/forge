# Forge Documentation Index

## Normative

- [Forge v1 Language Specification](forge-v1-spec.md) — source syntax and semantics.
- [FDN v1 Specification](fdn-v1-spec.md) — universal structured-data notation.
- [Compatibility Contract](compatibility.md) — what v1 promises long-term.

## Implementation

- [Grammar Sketch](grammar.ebnf) — parser-oriented EBNF.
- [Compiler Architecture](compiler-architecture.md) — bootstrap phases and IR strategy.
- [Memory Model](memory-model.md) — allocators, arenas, pools, scratch and collection ownership.
- [Concurrency](concurrency.md) — threads, CSP, agents and atomics.
- [Implementation Roadmap](implementation-roadmap.md) — milestone order.
- [Language Decisions](language-decisions.md) — concise rationale for frozen choices.

## Frontend implementation

- `frontend-ir.md` — AST/HIR/FIR boundary and comparison with other compiler frontends.
- `parser-status.md` — current Rust parser coverage and next grammar work.
- `parser-technology.md` — why the bootstrap uses Logos + Chumsky and alternatives considered.
