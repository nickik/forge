# Bootstrap Compiler Architecture

## Goals

The first compiler must be small enough to audit and modify quickly while preserving a clean path to strong optimization later. Compile-time speed and predictable memory use matter.

## Pipeline

```text
source
  -> Logos lexer
  -> Chumsky parser
  -> source-faithful AST
  -> HIR (reader expansion + name resolution + desugaring)
  -> typed HIR (type checking + constant evaluation)
  -> pattern decision trees
  -> FIR typed three-address IR
  -> CFG/basic blocks
  -> local/global-light optimization
  -> register allocation
  -> backend lowering
  -> assembly/object or portable C
```

FDN is parsed by a sibling reader and reused by metadata, package files, documentation and `#tag` payloads.

## Lexer

The Rust bootstrap uses Logos to generate a deterministic lexer. This supersedes the original hand-written-scanner bring-up plan. The lexer recognizes identifiers, keywords, numeric/string/character literals, punctuation, comments, FDN reader-tag introducers, and metadata introducers. It does not perform semantic macro expansion.

## Parser

- Chumsky combinators over the Logos token stream;
- precedence parsing for expressions (Pratt/folded precedence as appropriate);
- explicit synchronization at `;`, `}` and declaration starts after errors;
- no semantic name lookup or target lowering during parsing.

Reader forms are represented as structured AST nodes and expanded during a dedicated compile-time-reader phase, not by textual token replacement.

## AST

AST nodes preserve source spans and original syntax required for diagnostics. They are serializable for debugging/tooling. Do not encode inferred types, semantic IDs, or target-machine layout into AST types.

## HIR

Lower AST to a compiler-oriented HIR after reader expansion and name resolution. HIR should use declaration IDs rather than textual lookup and may desugar source conveniences. A second typed-HIR stage records exact Forge types, resolved overloads, closure captures and safety checks before FIR lowering.

## Name resolution

Resolve modules and lexical scopes before full type checking. Overload sets are symbols containing multiple exact signatures.

## Type checking

No C-style numeric promotions. Contextual typing of untyped literals is permitted only when there is exactly one lossless intended type. User types never implicitly become their representation type.

`Option` and `Result` are compiler-recognized type constructors despite v1 not exposing user-defined generics.

## Patterns

Compile match matrices into decision trees. Prefer discriminants/length tests before payload comparisons. Exhaustiveness and unreachable-arm analysis happen before IR lowering.

## FIR

Forge Intermediate Representation (FIR) is the code-generation boundary. Use typed three-address operations and explicit control flow. Example:

```text
%1:u32 = add.checked %a, %b
%2:bool = lt %1, 10u32
br %2, bb_true, bb_false
```

Checks are explicit IR operations so optimizers can prove them redundant without changing source semantics.

IR should include:

- checked/wrapping arithmetic as distinct operations;
- checked slice/array indexing;
- `Option`/tag operations;
- calls and closure calls;
- stack allocations;
- raw/unsafe memory operations;
- defer cleanup edges after lowering structured scopes.

## Initial optimization

Do not require SSA in v1 bootstrap.

Implement in this order:

1. constant folding;
2. algebraic simplification;
3. local value numbering;
4. copy propagation;
5. dead-code elimination;
6. basic block simplification;
7. liveness;
8. simple redundant bounds-check elimination;
9. loop-range check hoisting where obvious.

Add SSA later as an optional optimizer if measured workloads justify it.

## Register allocation

Start with linear scan. Add:

- live-interval splitting;
- rematerialization of constants/addresses;
- target register-class awareness;
- preferred register pairs where targets need them;
- target-specific peephole cleanup.

## Backends

Backend interface consumes typed target-independent IR plus target data-layout information.

Initial targets:

1. portable C emission for bring-up and differential testing;
2. a simple native backend suitable for validating direct code generation;
3. historical DEC/Lighting targets as project goals.

Never let C backend undefined behavior leak into Forge semantics: emit defensive code where C would otherwise have different semantics.

## Testing strategy

- lexer golden tests;
- parser positive/negative tests;
- type-check diagnostics;
- pattern exhaustiveness tests;
- compile-and-run semantic tests;
- differential execution between C and native backends;
- FDN round-trip/canonicalization tests;
- fuzz lexer/parser after stable core exists.
