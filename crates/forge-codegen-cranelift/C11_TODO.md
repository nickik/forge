# C11 — globals and static data

Goal: extend the C10 native object pipeline to represent, emit, access, initialize, link, and execute Forge globals without introducing a second layout or ABI policy.

## C11a — global object model

- [ ] add globals to the prepared-module/object plan
- [ ] define deterministic global symbol naming and linkage
- [ ] derive global size/alignment only through the authoritative C9 layout engine
- [ ] distinguish compile-time constants from runtime-initialized globals
- [ ] classify global storage intent for later `.rodata` / `.data` / `.bss` emission
- [ ] validate missing initializer plans, duplicate/stale symbols, illegal layouts, and initializer ordering
- [ ] add deterministic AArch64/RV64 planning tests

## C11b — static data emission

- [ ] emit immutable constants into `.rodata`
- [ ] emit initialized writable storage into `.data`
- [ ] emit zero/uninitialized storage into `.bss`
- [ ] serialize scalar and aggregate constants according to C9 layout
- [ ] support object relocations inside global data where pointers/function addresses require them
- [ ] verify ELF sections, alignment, symbols, and deterministic bytes on AArch64/RV64

## C11c — global access lowering

- [ ] implement FIR `LoadGlobal`
- [ ] generate global-address relocations in CLIF/object code
- [ ] support scalar globals first, then aggregate globals
- [ ] ensure global aggregates use normal C9 memory representation
- [ ] test functions reading globals through linked executables

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
