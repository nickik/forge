# C12 — native compiler integration

Goal: turn the C3-C11 AArch64 backend into a source-level Forge compiler path and execute the existing run conformance suite as real native programs.

## C12a — source compiler driver

- [x] add a top-level `forge-compiler` crate and `forgec` binary
- [x] drive source through parse -> HIR -> typed HIR -> FIR -> Cranelift
- [x] preserve the existing `forge-fir` / codegen semantic boundary
- [x] support `--check`, `--emit-object`, `--build`, and `--run`
- [x] accept the existing `forge-build` prefix options and diagnose C12c-only `--library` use explicitly
- [x] require and export `fn main() -> i32` for hosted executables
- [x] emit real AArch64 ELF objects from source
- [x] activate the existing ten `:run` conformance fixtures on native AArch64 Linux

C12a intentionally remains one source module. Semantic import/library linking is C12c rather than a textual-inclusion shortcut.

## C12b — hosted startup/runtime

- [x] link generated AArch64 objects with a minimal hosted startup shim
- [x] call the C11d synthetic module initializer before `main` when one exists
- [x] return Forge `main()`'s `i32` as the host process exit status
- [x] provide the canonical non-returning `__forge_panic` hosted hook
- [x] pass `forgec --run` program arguments through to the native process for later `std.args` provider work
- [x] add native link/execute tests in the compiler crate

C12b is deliberately the minimal hosted runtime boundary needed for native execution. General external runtime/provider symbol imports, hosted `std`, and multi-module linking remain later work.

## C12c — semantic source libraries

- [x] accept repeatable `forgec --library NAME=PATH` inputs
- [x] parse every source module independently before linking
- [x] resolve imported public functions and values through module-qualified paths
- [x] resolve imported public types through module-qualified type paths
- [x] reject cross-module access to private definitions
- [x] namespace library definitions deterministically so modules cannot collide after semantic linking
- [x] preserve lexical local shadowing while rewriting module-level references
- [x] support transitive source-library imports with cycle and missing-library diagnostics
- [x] feed `forge` package dependencies to `forgec` as dependency-ordered library inputs
- [x] keep one authoritative HIR -> typed HIR -> FIR -> Cranelift pipeline after semantic module linking
- [x] verify native AArch64 execution of a program using an imported library
- [x] verify full repository formatting/check/tests/conformance/Clippy remain green

C12c deliberately links parsed module structure, not concatenated source text. A dependency package must currently expose exactly one `:library` target. External binary ABI/provider imports and independently compiled reusable object/archive libraries remain later work.

C12c verification:

- direct `forgec --library` compiler/native tests: targeted run `35017976182`
- real two-package `forge` graph build/run on native AArch64: run `35018251629`
- full repository CI on the cleaned implementation tree: run `35018415860`
- conformance remains `128 passed; 0 failed; 3 pending`

## Completion gate

C12a/C12b are complete when the full repository CI is green on the native AArch64 runner and the ten existing source-level `:run` conformance fixtures compile, link, and execute through `forge-compiler`, not through hand-authored FIR harnesses.

C12c is complete when a package can import a dependency library through normal Forge `import` syntax, use its public functions and types, compile and execute natively through both direct `forgec --library` and the `forge` package graph, while private symbols remain inaccessible and the full repository CI stays green.
