# C11 — globals and static data

Goal: extend the C10 native object pipeline to represent, emit, access, initialize, link, and execute Forge globals without introducing a second layout or ABI policy.

## C11a — global object model

- [x] add globals to the prepared-module/object plan
- [x] define deterministic global symbol naming and linkage
- [x] derive global size/alignment only through the authoritative C9 layout engine
- [x] distinguish compile-time constants from runtime-initialized globals
- [x] classify global storage intent for later `.rodata` / `.data` / `.bss` emission
- [x] validate missing initializer plans, duplicate/stale symbols, illegal layouts, and initializer ordering
- [x] add deterministic AArch64/RV64 planning tests

C11a completion record: CI run `34993211028` passed formatting, workspace check, build-system tests, generated collections snapshot, frontend tests, workspace tests, conformance, and Clippy. `PreparedModule` retains functions, prepared globals, and global initializer order; `ObjectModulePlan` retains function and global symbols plus initializer order.

## C11b — static data emission

- [x] emit immutable constants into `.rodata`
- [x] emit initialized writable storage into `.data`
- [x] emit zero/uninitialized storage into `.bss`
- [x] serialize scalar and aggregate constants according to C9 layout
- [x] support object relocations inside global data where pointers/function addresses require them
- [x] verify ELF sections, alignment, symbols, and deterministic bytes on AArch64/RV64

C11b completion record: final CI run `35008041414` on tested implementation head `ca3624ed2a0d173aff7ea8739889462e29fb44a6` passed the full formatting/check/test/conformance/Clippy gate. Static globals are emitted as real ELF `.rodata`, `.data`, and `SHT_NOBITS` `.bss` sections with C9-derived size/alignment, deterministic `STT_OBJECT` symbols, scalar/aggregate serialization, and `.rela.rodata` / `.rela.data` symbol-address relocations. Native AArch64 and RV64/QEMU linked execution tests both pass.

## C11c — global access lowering

- [x] implement FIR `LoadGlobal`
- [x] generate global-address relocations in CLIF/object code
- [x] support scalar globals first, then aggregate globals
- [x] ensure global aggregates use normal C9 memory representation
- [x] test functions reading globals through linked executables

C11c completion record: final CI run `35008041414` on tested implementation head `ca3624ed2a0d173aff7ea8739889462e29fb44a6` passed the full formatting/check/test/conformance/Clippy gate. Functions now lower scalar and aggregate `LoadGlobal` through symbolic global addresses, object emission resolves `.text` relocations against global symbols, aggregate reads materialize into ordinary C9 memory representation, and linked execution passes on native AArch64 and RV64/QEMU.

## C11d — runtime global initialization

- [ ] compile `FirGlobalInitializer.function`
- [ ] honor initializer dependencies and `global_init_order`
- [ ] generate one module initialization entry point
- [ ] store initializer results into their global storage
- [ ] verify dependent initializers execute exactly once and in specified order
- [ ] link and execute initializer tests on native AArch64 and RV64/QEMU

## Completion gate

C11 is complete only when a Forge FIR module containing constant globals, aggregate globals, and dependent runtime-initialized globals can be emitted into a real relocatable object, linked, initialized, and executed correctly on native AArch64 with RV64/QEMU coverage, while the full formatting/check/test/conformance/Clippy gate remains green.

C11 does not redefine C9 layout/ABI policy and does not include closures or varargs.
