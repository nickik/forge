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

## Completion gate

C12a/C12b are complete when the full repository CI is green on the native AArch64 runner and the ten existing source-level `:run` conformance fixtures compile, link, and execute through `forge-compiler`, not through hand-authored FIR harnesses.
