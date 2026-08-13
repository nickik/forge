# Forge frontend representations

## Decision

The Rust parser emits a **source-faithful AST** with byte spans. It does not emit LLVM IR directly.

The planned frontend pipeline is:

```text
Forge source
  -> spanned tokens
  -> AST                      source-faithful, untyped
  -> HIR                      names resolved, syntax sugar removed
  -> typed HIR                every expression has a Forge type
  -> FIR                      typed, target-independent 3-address CFG IR
  -> LLVM IR / portable C / direct backend
```

The AST is a tooling boundary. It is designed for diagnostics, documentation, formatting, source analysis, and subsequent semantic lowering. It deliberately retains distinctions such as `val` vs `var`, source-level type syntax, calls, structure initializers, and exact byte spans.

The AST is serializable with `serde`, and `forge-parse --json file.fg` is the first machine-readable frontend interface.

## Why not lower AST directly to LLVM?

LLVM IR is intentionally low-level. Forge still needs to resolve names, choose exact overloads, enforce no implicit conversions, establish `Option`/`Result` representations, compile pattern matching, insert bounds/overflow checks, lower `defer`, and determine closure environments. Encoding those decisions while still in the syntax AST would couple parsing, semantic analysis, and backend layout.

Forge should therefore have at least one compiler-owned semantic representation before LLVM.

## Comparable frontend designs

### Clang

Clang parses and semantically analyzes source into an AST, then converts that source-level representation to LLVM IR. Clang's AST intentionally resembles written C/C++ closely, which also makes it useful for source tooling.

Forge follows this separation, but plans a typed HIR/FIR layer because Forge has significant desugaring and safety semantics that are useful to preserve above LLVM.

### Rust

`rustc` lowers parsed syntax through AST/HIR and then through THIR/MIR before code generation. HIR is a compiler-friendly representation with some surface syntax desugared; MIR is sufficiently low-level for control-flow analysis and optimizations before conversion to LLVM IR.

Forge's intended HIR/FIR split is closer to Rust's HIR/MIR split than to direct AST-to-LLVM lowering.

### Zig

Zig uses multiple compiler representations. Its AIR source describes AIR as "Analyzed Intermediate Representation", produced by semantic analysis and consumed by code generation, with one AIR instance per function. That is a useful model for Forge FIR: codegen should consume a semantically complete, typed, target-independent function representation rather than syntax nodes.

### Swift / ClangIR lesson

More sophisticated compilers increasingly retain a language-aware IR above LLVM because some transformations are easier before source semantics are erased. Forge should keep FIR intentionally much smaller than SIL or MLIR/CIR, but preserve Forge-specific operations such as checked arithmetic, checked indexing, `Option`, `Result`, closure calls, and defer cleanup until they are deliberately lowered.

## AST requirements

Every AST node that can produce a diagnostic carries a byte span.

AST nodes should:

- preserve source distinctions;
- own no target ABI decisions;
- contain no LLVM types;
- contain no inferred types;
- be stable enough for parser tests, but not part of the 40-year language ABI;
- serialize to JSON for debugging and external tooling;
- allow error nodes/recovery later without redesigning semantic passes.

## HIR requirements

HIR is created after module/name resolution and reader-form expansion.

It should:

- identify declarations by stable IDs rather than strings;
- desugar simple syntax (`for`, named calls, reader expansions where applicable);
- preserve pattern trees until exhaustiveness analysis;
- record overload candidate sets before exact selection;
- represent `T?` explicitly as the compiler-recognized `Option` type;
- retain source spans for diagnostics.

## Typed HIR requirements

After type checking:

- every expression has an exact Forge type;
- conversions are explicit nodes;
- overloads are resolved;
- `unsafe` authorization is recorded;
- closure capture sets and environment requirements are known;
- allocation and effect information can be attached for analysis.

## FIR requirements

FIR is the first representation intended primarily for code generation and optimization.

It should be function-local typed three-address code with explicit basic blocks and operations such as:

```text
%3:u32  = add.checked %1, %2
%4:bool = bounds.check %index, %slice.len
%5:u32  = load %slice.data, %index
br %cond, bb1, bb2
```

Important distinct operations should include:

- checked / wrapping / saturating integer operations;
- checked indexing;
- normal and raw loads/stores;
- calls and closure calls;
- `Option` tests/unwraps;
- `Result` tests/unwraps;
- explicit cleanup edges generated from `defer`;
- pattern discriminant tests;
- safe references vs raw pointers where useful to optimization;
- stack allocation;
- calls to explicit allocators.

FIR does **not** need to be SSA in the first compiler. SSA can later be constructed from FIR as an optimization representation.

## Parser technology

Initial implementation uses:

- **Logos** for fast deterministic tokenization with source spans;
- **Chumsky** for recursive token parsing, precedence parsing, rich errors, and recovery;
- **Serde JSON** as a temporary/debug AST interchange.

This intentionally replaces the earlier bootstrap plan for a hand-written C++ parser. If parser-library compile time or maintenance becomes a measured problem, the AST boundary allows the implementation to be replaced without changing semantic passes.

## Primary references used for this decision

- Rust Compiler Development Guide: Overview and HIR chapters
- Clang Toolchain / Introduction to the Clang AST
- Zig `src/Air.zig`
- Chumsky 0.13 documentation and official Logos integration example
- Logos documentation
