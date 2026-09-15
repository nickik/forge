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

- [ ] establish cross-repository native compatibility coverage: Forge CI owns public CForge probes; full Cosmic native coverage belongs in Cosmic CI because Forge's workflow token cannot read the private Cosmic repository
- [x] compile and run the existing CForge arithmetic smoke case natively unchanged
- [x] compile the CForge Game of Life through its existing `bootstrap-support` package dependency
- [ ] compile and run the simplest Cosmic semantic tests which need no hosted/provider operations, starting with `address_space_minimal_test.fg`
- [ ] expand through capability, HandleSpace, mapping-table, AddressSpace-object, and other pure semantic tests
- [ ] for every failure, classify it as parser/type/FIR/codegen/module/runtime/provider rather than adding test-specific exceptions
- [ ] keep a native-vs-CForge result matrix so behavior remains differential where both runners apply

Initial compatibility probing found and fixed two source-compatibility assumptions in the production compiler: Cosmic-style `--library` inputs may use the full declared module name rather than only its short import alias, and provider identifiers such as `__forge_console_write` are valid identifiers while lone `_` remains the wildcard token.

## C13b — hosted providers required by existing programs

- [x] make `std.console` usable natively so the existing CForge Game of Life runs unchanged and produces its expected console output
- [x] make native string literals/`str` representation usable across provider calls
- [x] implement the `std.string` provider operations demanded so far by existing programs: numeric conversion/parsing, byte length/indexing, and value equality/inequality
- [ ] implement `std.args` and pass the existing `forge run -- ...` arguments through the hosted provider
- [ ] implement `std.time`
- [ ] implement `std.fs`
- [ ] implement `std.lock`
- [x] keep platform implementation details outside portable Forge `std` source

A general external/native ABI is not a standalone prerequisite. The native path now supports reachable hosted `__forge_*` provider imports over the existing C9 call ABI. Only providers reachable from the program are emitted as undefined object symbols and supplied by the hosted runtime; ordinary C12c namespaced Forge functions remain normal compiled functions.

## C13c — native collections and CForge applications

- [x] identify the bootstrap collection modules already exercised by existing CForge/Forge tests
- [x] provide the raw typed-storage primitives required by the existing bootstrap collection corpus (`raw_u8`, `raw_u64`, `raw_usize`, and `raw_string`) using a native hosted implementation
- [x] run the existing `collections-bootstrap-smoke` unchanged through `forgec`
- [x] keep the existing CForge application example (Game of Life) running natively without changing its public Forge API
- [ ] add `raw_u32` when an existing corpus consumer requires it; there is no current CForge/Forge use of that bootstrap provider
- [ ] bring CKV or the next largest existing hosted Forge application onto the native path once its required std/provider surface is available
- [x] keep bootstrap storage policy explicit: collection algorithms remain ordinary Forge while the temporary hosted raw-store boundary owns process-lifetime backing storage

C13c's current acceptance case exercises list growth/get/set/remove/swap-remove/truncate, `ListU64`, hash-set duplicate insertion/contains/removal, string-to-`usize` and string-to-`u64` maps, `u64`-to-`u64` maps, growth/rehash/tombstones, values above 32 bits, native `str` equality, and boolean `!`. It passed unchanged in ARM verification run `35030648743`. The same existing smoke is also a permanent `forge-compiler` native integration test so ordinary CI retains the coverage after the temporary staging workflows are removed.

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

1. CForge arithmetic/package smoke — native
2. Cosmic `address_space_minimal_test.fg`
3. other provider-free Cosmic semantic tests
4. CForge Game of Life with real `std.console` — native
5. native collections bootstrap — native
6. hosted std string/args/time/fs/lock as demanded by existing programs
7. broader Cosmic page-table/SIA host-model suite
8. CKV / larger Forge application coverage

## Completion gate

C13 is complete when the production compiler runs a substantial existing CForge/Cosmic corpus natively rather than only compiler-owned fixtures: CForge Game of Life and collection/application coverage, the provider-free Cosmic semantic suite, and the host-model Cosmic memory/MMU tests that do not require actual SIA instruction execution. CForge remains available as the differential oracle and the full Forge repository CI remains green.
