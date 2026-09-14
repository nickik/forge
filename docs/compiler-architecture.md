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

Metadata is lowered separately from expression/type syntax into one generic target-keyed metadata table. HIR assigns metadata to semantic targets (items, fields, impl methods, and future declaration-owned targets), and typed HIR carries the same table forward. Layout, optimizer, linker and tooling passes interpret the metadata names relevant to them; the frontend does not create one bespoke HIR field or node kind per metadata spelling.

## Name resolution

Resolve modules and lexical scopes before full type checking. Overload sets are symbols containing multiple exact signatures.

## Type checking

No C-style numeric promotions. Contextual typing of untyped literals is permitted only when there is exactly one lossless intended type. User types never implicitly become their representation type.

`Option` and `Result` are compiler-recognized type constructors despite v1 not exposing user-defined generics.

Postfix `?` is resolved during type checking: its operand must be `Result[T, Ein]`, the enclosing function or closure must return `Result[R, Eout]`, and `Ein` must be assignable to `Eout`. Typed HIR records the resolved propagation edge explicitly so FIR never has to reconstruct `?` semantics from syntax.

Compile-time constants use a deliberately restricted semantic evaluator over pure literal/unary/binary expressions and references to compile-time `const` bindings. Module constants are memoized by `DefId` with dependency-cycle detection; local constants are retained by `LocalId` in typed bodies. These retained `ConstValue`s feed array lengths and explicit enum discriminants, so FIR never re-evaluates source expressions. Forge v1 does not execute arbitrary functions at compile time and has no general comptime interpreter.

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

The bootstrap FIR is now implemented in `forge-frontend::fir`. Each HIR expression has a stable body-local `ExprId`, and typed HIR retains exact function signatures, resolved receiver transformations, and named-argument parameter indices. FIR lowering consumes those semantic facts directly; it never matches source spans or re-runs overload/type resolution. FIR includes a verifier that rejects missing terminators, invalid block targets, return-type mismatches, and non-concrete semantic types.

The first lowering slice covers literals, locals/globals, direct and indirect calls, method auto-reference, explicit conversions, aggregates, checked/wrapping arithmetic, short-circuit boolean control flow, safe indexing with explicit bounds checks, assignments/places, `if`, `while`, C-style `for`, `foreach`, `break`/`continue`, `defer`, optional promotion, and `Result` propagation through explicit success/error CFG edges.

FIR deliberately diagnoses rather than guesses when an earlier semantic stage is incomplete. In particular, pattern decision trees, closure-environment semantics, materialized default call arguments, typed context overrides, and typed channel/select operations must be completed before those constructs can cross the FIR boundary.

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


## Semantic cleanup notes

- Fixed-array length is part of semantic `Ty::Array` and must survive into typed HIR/FIR.
- Impl methods receive real `DefId`s; typed calls resolve directly to the method/function `DefId`.
- Declaration-owned default expressions are typechecked before FIR and accumulate diagnostics with ordinary body errors.
- Exhaustiveness and provably unreachable arms for finite built-in/nominal sums (`bool`, optionals, enums, tagged unions) belong in semantic type checking because this is the first layer with both resolved type identity and typed patterns. The current check is deliberately conservative for guarded/refutable payload patterns; full pattern-matrix usefulness and decision-tree optimization remain a later pass.
- Map/collection pattern typing is deliberately deferred until Forge has a collection-pattern protocol. Preserve the HIR pattern shape; do not invent `Unknown`-driven semantics in FIR.
- Metadata remains one target-keyed table. Use the generic metadata query API instead of adding one field per attribute.

### Bitstruct v1 semantic decision

Keep bitstructs simple: storage is restricted to `u8`, `u16`, `u32`, or `u64`, and declared field widths must exactly fill the storage width (unused bits are written explicitly as reserved fields). Fields are assigned in declaration order starting at least-significant bit 0. A 1-bit field has source type `bool`; wider fields use the smallest ordinary unsigned Forge integer type that can represent their width. Forge does not create arbitrary-width integer types such as `u3` or `u5`. Reads zero-extend into the ordinary field type. Compile-time-known out-of-range writes are errors and dynamic writes are checked rather than silently truncated. Bit numbering is defined on the numeric storage value; target memory endianness remains the ordinary representation of that storage integer.
