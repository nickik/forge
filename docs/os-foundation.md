# Forge OS Foundation Profile

**Status:** design checklist before starting a production kernel repository.

The objective is not to finish the entire standard library before Cosmic begins. The objective is to stabilize the small freestanding substrate that kernel code would otherwise reimplement immediately and then depend on permanently.

## Evidence from Rust operating systems

Modern Rust kernels illustrate the same recurring foundation needs.

Redox's kernel uses `no_std`-friendly dependencies for fixed-capacity/small collections, bitfields/flags, hash tables, slab allocation, a linked-list allocator, spin synchronization, architecture support, and ELF/object parsing. Its kernel build uses aborting panic and explicitly warns about arithmetic overflow, unchecked indexing, pointer alignment, and casual unwrap/panic behavior.

Hubris separates the kernel ABI (`sys/abi`) from the task-side support library (`sys/userlib`). Its freestanding ecosystem relies heavily on fixed-capacity/heapless collections, bitfields/flags, byte ordering, critical sections, static assertions, zerocopy/layout-safe data handling, and architecture/embedded support.

Forge should provide the generic mechanisms once in `core`, while Cosmic-specific ABI, scheduler, IPC, VM, and architecture policy live in the Cosmic repository.

## Gate A — required in Forge `core` before serious kernel implementation

### 1. Panic/trap/runtime contract — already designed

Required:

- allocation-free `PanicInfo`;
- one non-returning freestanding panic provider;
- no required unwinder;
- stable panic kinds for bounds/overflow/divide/shift failures.

### 2. Memory allocation — underway

Required:

- explicit `Arena` capability (`context + ArenaOps`);
- explicit arbitrary-size `Allocator` capability;
- fixed-size `ObjectCache`;
- `MayWait` / `NoWait` propagation;
- reclaim;
- deterministic tests over kernel-like and hosted-like providers.

### 3. Raw memory and byte operations

Required APIs or compiler intrinsics:

```text
copy_nonoverlapping
move_overlapping
set_bytes
zero_bytes
compare_bytes
```

Safe slice wrappers should be added on top. The primitive implementation must work without libc.

### 4. Volatile/MMIO access

Kernel/driver code needs explicit operations such as:

```text
volatile_load[T]
volatile_store[T]
```

They require `unsafe`, preserve target ordering rules, and must never be optimized into ordinary cached loads/stores.

### 5. Atomics and barriers

At minimum:

```text
AtomicU32 / AtomicU64 / AtomicUsize
load/store
compare_exchange
fetch_add/sub/and/or/xor
memory-order enum
compiler fence
CPU/thread fence
```

Target support may vary, but unsupported atomic widths must be diagnosed rather than silently emulated in unsafe ways.

### 6. Minimal synchronization building blocks

`core` should contain policy-neutral mechanisms, not an OS scheduler:

```text
SpinLock
Once/InitCell-like one-time initialization
critical-section capability/hook
```

Blocking mutexes, condition variables, futexes, and scheduler integration belong in Cosmic/std.

### 7. Bit, mask and endian utilities

Required very early for page tables, device registers, packet formats and ABI records:

```text
rotate/count bits
bit extraction/insertion helpers
checked shifts
LE/BE load/store helpers
byte swap
bit flags without hidden allocation
```

### 8. Fixed-capacity collections

Before a heap is reliable, kernel bootstrap and interrupt paths need bounded containers:

```text
FixedVec[T,N]
FixedString[N]
RingBuffer[T,N]
```

Forge v1 lacks user-defined generics, so these will likely be compiler/reader-generated concrete types or standard generated families rather than ordinary parametric source types.

### 9. Intrusive collections

A kernel frequently needs objects to carry their own linkage:

```text
intrusive singly/doubly linked list
intrusive queue
```

This avoids separate node allocation and maps naturally to scheduler queues, free lists, cache lists and VM structures.

### 10. Layout and static assertions

Kernel ABI and hardware structures need compile-time guarantees:

```text
sizeof
alignof
offsetof
static_assert
@repr(c)
@repr(packed) where explicitly required
```

Layout failures must be compile errors.

### 11. Non-allocating debug output

Define a minimal `Writer` capability plus integer/hex/string formatting that can write into:

```text
serial console
framebuffer console
memory log
host stderr
```

without requiring a `String`, allocator, filesystem, or process model.

### 12. Target facts

A small target API should expose compile-time facts such as:

```text
pointer width
endianness
page-size convention where target-defined
architecture identifier
cache-line hint where available
```

Architecture-specific register/interrupt/page-table operations stay in Cosmic architecture modules.

## Gate B — required in the Cosmic repository, not Forge `core`

These should be designed alongside the first kernel but should not be standardized as generic Forge facilities:

```text
cosmic.arch       trap/interrupt entry, CPU registers, paging
cosmic.boot       firmware/bootloader handoff
cosmic.abi        stable kernel/user ABI records and syscall numbers
cosmic.user       userspace syscall/IPC support library
cosmic.vm         physical/virtual memory policy
cosmic.sync       scheduler-aware mutex/wait queues
cosmic.ipc        capabilities/endpoints/messages
cosmic.task       task/process/thread model
```

Hubris's explicit split between `sys/abi` and `sys/userlib` is a useful precedent: the kernel ABI should be independently consumable by user programs and tools, rather than being defined inside kernel-private code.

## Gate C — may wait until after the first kernel boots

Do not block kernel bring-up on:

```text
hash maps
full dynamic strings
ELF/general object loader
filesystem abstractions
network stack
crypto
locale/Unicode machinery
high-level formatting
hosted process API
```

Redox needs several of these today because it is a mature general-purpose OS; that does not mean Cosmic needs them before its first scheduler/IPC/VM work.

## Testing requirement

Every Gate A facility should have two classes of tests:

1. **host/reference tests** — deterministic, exhaustive edge cases where practical;
2. **bare-metal integration tests** — boot under QEMU/emulator and exercise the same API with real architecture/runtime providers.

The first Cosmic repository should include a test kernel from day one. A test failure must be able to report through the non-allocating Writer/panic path and terminate QEMU deterministically.

## Recommended start condition for Cosmic

Cosmic proper can begin once these are stable enough to depend on:

```text
panic/runtime ABI
Arena + Allocator + ObjectCache
raw memory operations
volatile/MMIO
atomics + SpinLock/critical-section primitive
bit/endian helpers
FixedVec + RingBuffer
intrusive list
layout/static assertions
non-allocating Writer/debug formatting
```

The rest should be built in Cosmic as real kernel requirements appear rather than guessed in advance.
