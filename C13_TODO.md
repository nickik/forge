# C13 — native execution of existing Forge code

Goal: make the production native Forge compiler run as much of the existing CForge and Cosmic Forge code/test suites as possible. C13 is driven by real existing programs, not by designing a general foreign-function ABI in isolation.

## Principles

- Existing CForge/Cosmic Forge source is the compatibility target; avoid rewriting tests merely to suit the native backend.
- Add compiler/runtime/provider machinery only when an existing program requires it.
- Keep CForge as the semantic/reference implementation while native Forge coverage grows.
- Prefer portable Forge implementations for library algorithms; keep OS/platform effects behind provider boundaries.
- Preserve the C9 layout/ABI as the single representation authority.
- Run native AArch64 first; mirror suitable coverage on RV64/QEMU as support becomes available.

## C13a — compile existing pure-Forge code

- [ ] establish a cross-repository native compatibility harness that checks out CForge and Cosmic and invokes the production `forgec`
- [ ] compile and run the CForge arithmetic/package smoke cases natively
- [ ] compile the CForge Game of Life through its existing `bootstrap-support` package dependency
- [ ] compile and run the simplest Cosmic semantic tests which need no hosted/provider operations, starting with `address_space_minimal_test.fg`
- [ ] expand through capability, HandleSpace, mapping-table, AddressSpace-object, and other pure semantic tests
- [ ] for every failure, classify it as parser/type/FIR/codegen/module/runtime/provider rather than adding test-specific exceptions
- [ ] keep a native-vs-CForge result matrix so behavior remains differential where both runners apply

## C13b — hosted providers required by existing programs

Implement only providers exercised by existing CForge/Forge applications, in this order unless the compatibility harness shows a better dependency order:

- [ ] make `std.console` usable natively so the existing CForge Game of Life runs unchanged and matches its golden output
- [ ] make native string literals/`str` representation usable across provider calls
- [ ] implement the `std.string` operations currently supplied by CForge when an existing program requires them
- [ ] implement `std.args` and pass the existing `forge run -- ...` arguments through the hosted provider
- [ ] implement `std.time`
- [ ] implement `std.fs`
- [ ] implement `std.lock`
- [ ] keep platform implementation details outside portable Forge `std` source

A general external/native ABI is not a standalone prerequisite. If provider calls need undefined symbols or a stable call boundary, implement the minimum correct mechanism as part of the provider path and generalize only once real callers require it.

## C13c — native collections and CForge applications

- [ ] identify the generated/native collection modules already consumed by CForge tests
- [ ] provide the raw typed-storage primitives currently modeled by CForge (`raw_u8`, `raw_u32`, `raw_u64`, `raw_usize`, and `raw_string`) using a native hosted implementation
- [ ] run existing collection bootstrap/integration tests through `forgec`
- [ ] compile and run the existing CForge application examples without changing their public Forge APIs
- [ ] bring CKV or the next largest existing hosted Forge application onto the native path once its required std/provider surface is available
- [ ] keep allocation/storage policy explicit and compatible with Forge's existing allocator design

## C13d — Cosmic semantic suite on native host

Migrate in increasing dependency order rather than attempting the whole kernel at once:

- [ ] pure logical AddressSpace/capability/HandleSpace/object-registry tests
- [ ] mapping and MemoryObject tests
- [ ] page-table structures using a native host page-table-memory provider
- [ ] SIA architectural state model on the host
- [ ] TLB/ASID/address-space activation tests
- [ ] later trap/task/IPC/signal semantic tests as their dependencies become native-compatible

The native-host lane validates Forge/Cosmic algorithms. It does not replace LightingSimulation for real SIA instruction/MMU validation.

## C13e — differential CI

- [ ] keep CForge reference execution for compatible tests
- [ ] add native AArch64 execution beside it
- [ ] compare exit status and stdout/stderr for application tests
- [ ] compare semantic pass/fail vectors for Cosmic tests
- [ ] enable RV64/QEMU for tests that do not depend on AArch64-host providers
- [ ] keep CI bounded: group tests into a small number of matrix jobs rather than one workflow per source file

## Initial acceptance ladder

1. CForge arithmetic/package smoke
2. Cosmic `address_space_minimal_test.fg`
3. other provider-free Cosmic semantic tests
4. CForge Game of Life with real `std.console`
5. native collections bootstrap
6. hosted std string/args/time/fs/lock as demanded by existing programs
7. broader Cosmic page-table/SIA host-model suite
8. CKV / larger CForge application coverage

## Completion gate

C13 is complete when the production compiler runs a substantial existing CForge/Cosmic corpus natively rather than only compiler-owned fixtures: CForge Game of Life and collection/application coverage, the provider-free Cosmic semantic suite, and the host-model Cosmic memory/MMU tests that do not require actual SIA instruction execution. CForge remains available as the differential oracle and the full Forge repository CI remains green.
