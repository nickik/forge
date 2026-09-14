# Forge Documentation Index

## Normative

- [Forge v1 Language Specification](forge-v1-spec.md) — source syntax and semantics.
- [Forge v1 Library and Execution Environment Specification](forge-v1-library-spec.md) — `core`, `std`, freestanding/hosted builds, runtime ABI boundary, and compilation-unit model.
- [Forge Runtime ABI](runtime-abi.md) — canonical panic/trap provider contract for hosted and freestanding final artifacts.
- [FDN v1 Specification](fdn-v1-spec.md) — universal structured-data notation.
- [Compatibility Contract](compatibility.md) — what v1 promises long-term.

## Standard library design

- [Library Model](library-model.md) — design rationale and bootstrap module/library graph.
- [Core Library](core-library.md) — freestanding facilities shared by kernel and user mode.
- [Core Allocation Architecture](core-allocation.md) — explicit provider capabilities, variable-sized allocators, fixed-size object caches, reclaim, and kernel/user reuse.
- [OS Foundation Profile](os-foundation.md) — the minimal `core` facilities to stabilize before a production kernel depends on Forge.
- [Freestanding Library Roadmap](freestanding-libraries.md) — `--no-std` libraries for memory, bits, MMIO, atomics, bounded containers, intrusive collections, layout, output, and target facts, with per-library TODOs and the pre-Cosmic integration gate.

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
