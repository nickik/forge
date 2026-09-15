# Bootstrap Compiler Architecture

## Goals

The compiler must stay small enough to audit and modify quickly while preserving a clean path to strong optimization. Compile-time speed, deterministic behavior, and predictable memory use matter.

## Pipeline

```text
source
  -> Logos lexer
  -> Chumsky parser
  -> source-faithful AST
  -> HIR (reader expansion + name resolution + desugaring)
  -> typed HIR (type checking + resolved Forge semantics)
  -> FIR (typed explicit control flow; final Forge semantic IR)
  -> forge-fir (frozen backend-facing FIR surface)
  -> FIR -> CLIF (mechanical code-generation lowering)
  -> Cranelift IR
  -> Cranelift optimization/legalization/instruction selection/register allocation
  -> AArch64 / RISC-V64 machine code
  -> SIA machine code later through a Cranelift ISA backend
```

FDN is parsed by a sibling reader and reused by metadata, package files, documentation and `#tag` payloads.

## Lexer

The Rust bootstrap uses Logos to generate a deterministic lexer. The lexer recognizes identifiers, keywords, numeric/string/character literals, punctuation, comments, FDN reader-tag introducers, and metadata introducers. It does not perform semantic macro expansion.

## Parser

- Chumsky combinators over the Logos token stream;
- precedence parsing for expressions;
- explicit synchronization at `;`, `}` and declaration starts after errors;
- no semantic name lookup or target lowering during parsing.

Reader forms are represented as structured AST nodes and expanded during a dedicated compile-time-reader phase, not by textual token replacement.

## AST

AST nodes preserve source spans and original syntax required for diagnostics. They are serializable for debugging/tooling. Do not encode inferred types, semantic IDs, or target-machine layout into AST types.

## HIR and typed HIR

Lower AST to compiler-oriented HIR after reader expansion and name resolution. HIR uses declaration IDs rather than textual lookup and may desugar source conveniences. Typed HIR records exact Forge types and resolves language semantics before FIR lowering.

This includes overloads, receiver transformations, named/default arguments, pattern/match plans, closure captures, unsafe authorization, context slots, collection-pattern protocols, channel/select semantics, global-initializer dependencies and other source-language decisions.

Metadata is lowered separately into one generic target-keyed metadata table. Layout, optimizer, linker and tooling passes interpret metadata names relevant to them; the frontend does not create one bespoke HIR field or node kind per metadata spelling.

## Name resolution and type checking

Resolve modules and lexical scopes before full type checking. Overload sets are symbols containing multiple exact signatures.

No C-style numeric promotions. Contextual typing of untyped literals is permitted only when there is exactly one lossless intended type. User types never implicitly become their representation type.

`Option` and `Result` are compiler-recognized type constructors despite v1 not exposing user-defined generics.

Postfix `?` is resolved during type checking. Typed HIR records the resolved propagation edge explicitly so FIR never reconstructs `?` semantics from syntax.

Compile-time constants use a restricted semantic evaluator over pure literal/unary/binary expressions and references to compile-time `const` bindings. Retained constant values feed array lengths and explicit enum discriminants. FIR never re-evaluates source expressions.

## Patterns

Pattern usefulness, exhaustiveness, unreachable-arm analysis and semantic match planning happen before FIR. FIR receives explicit tests, projections, bindings and control-flow requirements rather than source patterns that still need interpretation.

## FIR

Forge Intermediate Representation (FIR) is the **final Forge-owned semantic IR and the portable code-generation boundary**. It uses typed operations and explicit control flow. For example:

```text
%1:u32 = add.checked %a, %b
%2:bool = lt %1, 10u32
br %2, bb_true, bb_false
```

Checks are explicit IR operations so later optimization can prove them redundant without changing Forge semantics.

The FIR boundary is hardened: successful typed HIR must contain concrete types and resolved semantic shapes, and emitted FIR is verified as a whole module. FIR lowering diagnoses incomplete earlier semantic work rather than guessing.

`forge-fir` is the frozen backend-facing package surface. Backend crates depend on `forge-fir`, not on `forge-frontend`; the facade exports FIR plus only the foundational IDs and types that appear in FIR fields. AST, HIR, Typed HIR, parser, resolver and type-checker implementation structures are not backend APIs.

## FIR -> CLIF boundary

**CLIF is a code-generation IR, not a new Forge semantic compiler layer.**

The `forge-codegen-cranelift` crate consumes verified `FirModule` data exclusively through `forge-fir`. FIR -> CLIF may choose Cranelift representations required to express an already-resolved FIR operation, but it must not perform or reconstruct Forge language semantics.

In particular it must not redo:

- name or overload resolution;
- named/default argument normalization;
- pattern usefulness or match planning;
- closure capture discovery;
- unsafe authorization;
- execution-context resolution;
- collection protocol selection;
- channel/select semantic planning.

Unsupported FIR is an explicit backend error. Codegen must never inspect AST, HIR or Typed HIR to recover missing information.

## Optimization

Forge may retain small, clearly semantic-independent FIR simplifications when they are useful for diagnostics or deterministic canonicalization. We do not build a second general optimizer or SSA pipeline below FIR.

Cranelift owns the code-generation optimization layer: CLIF SSA optimization, legalization, instruction selection and target-specific lowering. This avoids duplicating mature compiler infrastructure in Forge.

## Register allocation

Forge does not implement a separate register allocator. Cranelift owns virtual-register lowering, liveness/spilling and register allocation for AArch64, RISC-V64 and the future SIA backend.

SIA-specific register classes and allocation constraints therefore belong in the SIA Cranelift backend, not in FIR or FIR -> CLIF lowering.

## Backends

Initial execution targets are:

1. **AArch64** — native local execution, especially on Apple Silicon;
2. **RISC-V64** — independent existing Cranelift target for differential testing and emulation;
3. **SIA** — later, implemented as a normal Cranelift ISA backend after FIR -> CLIF coverage is mature.

The architecture is deliberately:

```text
                   -> Cranelift AArch64
Forge -> FIR -> CLIF -> Cranelift RISC-V64
                   -> Cranelift SIA      (later)
```

SIA is not a Forge-specific backend. Once implemented, it lives below CLIF and benefits from Cranelift's common backend infrastructure.

For the bootstrap, Forge pins the reduced `nickik/crainlift` fork at `fcd03035697e5f9b68bf674698ae0da128f4f5c7`. That fork removes the Wasmtime runtime, WASI, Winch, component model, Pulley and unrelated tooling while retaining the Cranelift dependency closure required for native code generation.

## Testing strategy

- lexer golden tests;
- parser positive/negative tests;
- type-check diagnostics;
- pattern exhaustiveness tests;
- FIR boundary and verifier tests;
- package-boundary tests preventing codegen from importing frontend semantic IR;
- FIR -> CLIF lowering tests for each supported FIR operation;
- Cranelift verifier checks on generated CLIF;
- compile-and-run semantic tests on AArch64;
- differential execution of the same FIR/CLIF behavior on RISC-V64;
- SIA joins the same differential suite later;
- FDN round-trip/canonicalization tests;
- fuzz frontend and code-generation boundaries after the stable core exists.

## Semantic cleanup notes

- Fixed-array length is part of semantic `Ty::Array` and must survive into typed HIR/FIR.
- Impl methods receive real `DefId`s; typed calls resolve directly to the method/function `DefId`.
- Declaration-owned default expressions are typechecked and normalized before FIR.
- Exhaustiveness and unreachable-arm analysis belong above FIR.
- Collection-pattern protocol selection belongs above FIR.
- Metadata remains one target-keyed table.

### Bitstruct v1 semantic decision

Bitstruct storage is restricted to `u8`, `u16`, `u32`, or `u64`, and declared field widths must exactly fill the storage width. Fields are assigned in declaration order starting at least-significant bit 0. A 1-bit field has source type `bool`; wider fields use the smallest ordinary unsigned Forge integer type that can represent their width. Reads zero-extend into the ordinary field type. Compile-time-known out-of-range writes are errors and dynamic writes are checked rather than silently truncated. Bit numbering is defined on the numeric storage value; target memory endianness remains the ordinary representation of that storage integer.
