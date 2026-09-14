# Forge `core` library

**Status:** initial v1 design baseline.

`core` is the freestanding standard library shared by hosted programs, kernels, firmware, boot environments, and tests. It contains algorithms and abstractions that do not require an operating system or a default heap.

## Design goals

1. The same fundamental code must work in kernel and user mode.
2. Allocation is explicit and fallible.
3. Environment-specific resources enter through explicit capability objects.
4. Fixed-size object allocation is first-class, while arbitrary-size allocation remains fully supported.
5. Panic/trap handling must remain usable before an allocator, scheduler, console, or filesystem exists.
6. `core` must not require libc, a process-global heap, or a conventional `main()`.

The allocation design is intentionally similar in spirit to Solaris slab/UMEM: object-cache algorithms are reusable while the environment supplies backing memory. It is also data-oriented/ECS-inspired in the limited sense that homogeneous object shapes can receive dedicated storage and lifecycle policy; `ObjectCache` itself is not an ECS.

## Core capability-object pattern

Forge v1 has no traits or interfaces. Dynamic provider abstraction uses ordinary data and function pointers:

```forge
struct ArenaOps {
    alloc: fn(*void, AllocRequest) -> Result[MemoryBlock, AllocError];
    free: fn(*void, MemoryBlock) -> void;
    reclaim: fn(*void, usize) -> usize;
}

struct Arena {
    context: *void;
    ops: &ArenaOps;
}
```

This is an explicit capability object:

```text
Arena
  context -> provider-specific state
  ops     -> static operations table
```

It is intentionally equivalent to explicit C-style dictionary/vtable passing, but there is no hidden object header, RTTI, inheritance, implicit allocation, or language-defined dynamic dispatch.

The pattern is suitable for other low-level abstractions such as `Writer`, `Clock`, interrupt controllers, or entropy sources when dynamic substitution is genuinely required. Static concrete calls remain preferable when the provider type is already known.

## Panic in freestanding environments

`core` defines `PanicKind`, `PanicLocation`, and `PanicInfo`, but not termination policy. All defined panic/trap paths converge on:

```text
__forge_panic(info: &core.PanicInfo) -> never
```

A hosted build receives the default provider from `std`. A `--no-std` final artifact supplies exactly one implementation itself.

Forge v1 panic is non-unwinding. It requires no exception tables, heap, scheduler, filesystem, or OS. Primitive allocation failure is not panic; allocation returns `Result`, and callers explicitly choose whether to retry, reclaim, propagate, degrade, or panic.

## Allocation architecture

```text
                     callers
                        |
             +----------+----------+
             |                     |
     arbitrary Allocator       ObjectCache
             |                 fixed objects
             +----------+----------+
                        |
                      Arena
                 context + ops
                        |
       +----------------+----------------+
       |                |                |
   Cosmic VM        hosted VM       static/test RAM
```

### Arena

`Arena` is the lower backing-resource capability. `core` knows only its operations table and provider context.

Normative operations:

```forge
arena_alloc(arena, request) -> Result[MemoryBlock, AllocError]
arena_free(arena, block) -> void
arena_reclaim(arena, target_bytes) -> usize
```

A kernel provider may obtain pages/address ranges from Cosmic VM. A hosted provider may use platform virtual memory. Firmware may use a fixed RAM range. Tests may use a deterministic fake address space.

`AllocWait::NoWait` must propagate unchanged to the provider and must never silently become a sleeping allocation.

### Arbitrary-size Allocator

`Allocator` uses the same capability-object pattern:

```forge
struct AllocatorOps {
    alloc: fn(*void, AllocRequest) -> Result[MemoryBlock, AllocError];
    free: fn(*void, MemoryBlock) -> void;
    resize: fn(*void, MemoryBlock, usize) -> Result[MemoryBlock, AllocError];
}

struct Allocator {
    context: *void;
    ops: &AllocatorOps;
}
```

This is the traditional malloc-like workload without a hidden global heap. Requests carry size, alignment, and wait policy explicitly.

A concrete general allocator may itself be implemented over an `Arena`, may use object caches as size classes, or may use another policy entirely.

### Fixed-size ObjectCache

`ObjectCacheSpec` describes one repeated object shape:

```forge
struct ObjectCacheSpec {
    object_size: usize;
    object_align: usize;
    slab_size: usize;
}
```

Geometry:

```text
stride = align_up(object_size, object_align)
objects_per_slab = slab_size / stride
```

Initial validity rules:

- object size is non-zero;
- object and slab alignment are powers of two;
- the aligned object stride fits inside the slab;
- the slab is large enough to hold at least one object.

The cache obtains slabs through `Arena`, subdivides them into equal-size objects, tracks free/in-use objects, grows on demand, and may return completely unused slabs during explicit reclaim.

Typical direct users include process/thread records, VFS nodes, packet descriptors, IPC endpoints, driver requests, compiler nodes, database records, ECS component chunks, and protocol objects.

A general allocator may also use several `ObjectCache` instances internally as size classes.

## Kernel/user sharing rule

> Algorithms and policy-neutral mechanisms belong in `core`; acquisition of pages/address space and scheduler/platform integration belong to the environment.

The same object-cache code must therefore pass against at least:

```text
kernel-like Arena provider
hosted/user-like Arena provider
fixed deterministic test provider
```

Provider identity must not affect cache semantics.

## Concurrency

The first implementation prioritizes correctness. Synchronization is not hidden inside the basic provider contract.

Later implementations may add environment-specific synchronization and Solaris-style per-CPU/per-thread magazines:

```text
per-CPU/thread magazine
        -> central depot
        -> slab/cache
        -> Arena
```

This must not alter the public object-cache contract.

## OS-foundation facilities that belong in `core`

Before a production kernel starts depending on Forge, the following freestanding pieces should be stabilized:

```text
panic/trap records and runtime hook
Arena / Allocator / ObjectCache
memory copy/move/set and byte spans
bit operations and endian conversion
volatile/MMIO primitives
atomics and memory ordering
critical-section / spin-lock building blocks
fixed-capacity vector/ring-buffer/string containers
intrusive list/queue primitives
Option/Result utilities
layout/offset/static assertions
non-allocating formatting/debug writer primitives
target/ABI facts
```

These are mechanism libraries shared by kernels, embedded targets, drivers, and hosted low-level code. Filesystems, sockets, process APIs, hosted threads, and OS virtual-memory syscalls stay out of `core`.

## Required testing

`core` must have executable reference tests for:

- identical Arena dispatch over kernel-like and hosted-like providers;
- arbitrary sizes and alignments;
- OOM as an error rather than panic;
- `NoWait` propagation;
- free and resize dispatch;
- object uniqueness while live;
- object free/reuse;
- slab growth;
- empty-slab reclaim without reclaiming live slabs;
- statistics invariants;
- invalid object-cache geometry;
- explicit provider reclaim dispatch;
- panic operation without allocator or unwinder.

The CForge suite supplies a deterministic provider/object-cache semantic model until both Forge implementations can execute the full raw-pointer implementation directly. Those model tests are not a substitute for later bare-metal integration tests; they establish the portable algorithm contract first.
