# Forge Freestanding Library Roadmap

**Status:** initial implementation baseline for Cosmic pre-OS work.

These libraries are ordinary Forge compilation units layered on `core`. They are usable with `--no-std`, must not require libc or an operating system, and must not hide allocation. Cosmic may import them, but none of them may import Cosmic.

## Dependency rule

```text
Forge language/runtime intrinsics
          |
        core
          |
   +------+------+------+------+------+------+------+------+------+
   |      |      |      |      |      |      |      |      |      |
  mem    bits   mmio  atomic  sync  fixed intrusive layout  io   target
```

`sync` may additionally depend on `atomic`. Higher-level freestanding libraries may compose these later, but cycles are not permitted.

## core.mem

Purpose: raw copying, moving, setting, comparison, byte spans.

- [x] `ByteSpan` contract
- [x] overlap/range helpers
- [x] copy/move/set/compare primitive contracts
- [ ] safe slice wrappers
- [ ] actual backend lowering tests
- [ ] poisoned/guarded debug variants where useful

## core.bits

Purpose: register fields, wire formats, page tables, storage formats.

- [x] `bswap_u16/u32`
- [x] low-mask/extract/insert helpers
- [ ] u64 variants
- [ ] rotates/popcount/clz/ctz
- [ ] endian loads/stores from byte slices
- [ ] property tests over all field widths

## core.mmio

Purpose: explicit volatile device-register access.

- [x] volatile u8/u16/u32/u64 load/store contracts
- [x] `MmioU32` wrapper
- [ ] target-specific device ordering specification
- [ ] QEMU fake-device integration tests
- [ ] explicit safe wrappers for typed register blocks

## core.atomic

Purpose: lock-free primitives independent of a scheduler.

- [x] memory-order enum
- [x] atomic u32 load/store/fetch/CAS contract
- [x] compiler and memory fences
- [ ] complete `AtomicUsize`
- [ ] legal ordering validation
- [ ] architecture litmus/codegen tests

## core.sync

Purpose: synchronization that does not require sleeping or a scheduler.

- [x] `SpinLock` contract
- [x] capability-style critical-section provider
- [ ] `Once`
- [ ] debug owner/recursion checks
- [ ] prove no scheduler/allocator dependency
- [ ] keep sleeping locks in Cosmic

## core.fixed

Purpose: bounded collections usable before/during allocator bring-up and in hard real-time paths.

- [x] `ByteVec256` layout and read-only helpers
- [x] `UsizeRing64` layout and read-only helpers
- [ ] Forge-source mutation operations
- [ ] generated `FixedVec_T_N`
- [ ] generated `RingBuffer_T_N`
- [ ] exhaustive boundary/wraparound tests

## core.intrusive

Purpose: allocation-free queues/lists for already-existing objects.

- [x] link/list layout contract
- [x] length/empty helpers
- [ ] pointer mutation implementation in Forge
- [ ] owner/container recovery primitive
- [ ] double-insert/foreign-remove debug checks
- [ ] queue wrapper and iteration

## core.layout

Purpose: alignment and ABI/layout assertions.

- [x] align up/down/is-aligned helpers
- [x] bootstrap contracts for size/alignment/offset/static assertion
- [ ] compile-time type operands instead of bootstrap IDs
- [ ] repr(c)/packed/alignment golden tests
- [ ] per-target ABI fixtures

## core.io

Purpose: non-allocating output for early boot, panic, firmware and hosted adapters.

- [x] explicit `Writer` capability
- [x] fallible write contract
- [ ] `write_str`
- [ ] decimal/hex/pointer formatting without heap allocation
- [ ] fixed-buffer writer
- [ ] serial/platform adapters stay outside this library

## core.target

Purpose: compile-time target facts without platform policy.

- [x] endian and architecture enums
- [x] target-info contract and pointer-width helper
- [ ] compile-time target predicates
- [ ] freeze whether page-size/cache-line hints belong here
- [ ] backend target-info tests

## Pre-Cosmic acceptance test

Before Cosmic relies on this layer, the conformance/reference harness must run one scenario that combines:

1. a kernel-like or hosted-like `Arena` provider;
2. `ObjectCache` allocation of fixed-size task records;
3. layout/alignment validation;
4. an intrusive runnable queue;
5. a bounded event ring;
6. atomic accounting;
7. MMIO-style register access;
8. endian/bitfield encoding;
9. non-allocating diagnostic output;
10. free/reclaim through the same Arena capability.

The exact same logical scenario must pass over both kernel-like and hosted-like providers. Later, the kernel-like provider is replaced by actual Cosmic page/VM code under QEMU without changing the upper-library algorithms.
