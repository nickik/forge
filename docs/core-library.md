# Forge `core` library

**Status:** initial v1 design baseline.

`core` is the freestanding standard library shared by hosted programs, kernels, firmware, boot environments, and tests. It contains algorithms and abstractions that do not require an operating system or a default heap.

## Goals

1. Make the same fundamental code usable in kernel and user mode.
2. Keep allocation explicit and fallible.
3. Separate allocation algorithms from the source of address space or physical memory.
4. Treat fixed-size objects as a first-class allocator workload.
5. Also support arbitrary-size malloc-like allocations through an explicit allocator.
6. Permit debugging, accounting, reclaim, and statistics without changing callers.
7. Avoid requiring libc or a process-global allocator.

The allocator architecture is intentionally similar in spirit to the Solaris slab/UMEM family: object caches manage fixed-size objects while a lower backing arena supplies larger extents. The same cache algorithms can therefore be reused with different backing providers in kernel and user environments.

It is also data-oriented/ECS-inspired in the limited sense that homogeneous object shapes receive dedicated storage and management policy. `ObjectCache` is not an ECS: it has no entity IDs, component composition, queries, or systems. It is a lower-level memory primitive that ECS/component stores can use.

## Initial `core` contents

The first single-file `core.fg` is intentionally compact, but the logical API is expected to grow into these areas:

```text
core
  panic/trap information
  integer/math helpers
  bit manipulation
  memory primitives
  slices and byte spans
  strings/views with no allocation
  Option/Result helpers
  target/layout information
  atomics once language/runtime support is ready
  Arena backing-resource abstraction
  Allocator arbitrary-size allocation
  ObjectCache fixed-size allocation
  fixed pools and arenas
```

## Panic in freestanding environments

`core` defines `PanicKind`, `PanicLocation`, and `PanicInfo`, but does not define termination policy. Every reachable panic/trap path funnels to the runtime ABI hook:

```text
__forge_panic(info: &core.PanicInfo) -> never
```

A hosted build receives the default implementation from `std`. A `--no-std` final artifact supplies exactly one implementation itself.

Forge v1 panic is non-unwinding. It does not require exception tables, an unwinder, a heap, a scheduler, or an OS. This makes the same checked language semantics usable in kernel and embedded code.

Primitive allocation failure is not a panic. Allocation returns `Result`; callers explicitly choose whether to propagate, reclaim/retry, fall back, or convert failure into `PanicKind::AllocationFailure`.

See `runtime-abi.md` for the full contract.

## Allocation model

Forge does not define `malloc` as a language primitive. Allocation happens through an explicit allocator or an owning object.

There are two peer upper allocation styles over a common resource layer:

```text
                        callers
                           |
               +-----------+-----------+
               |                       |
       arbitrary-size Allocator    fixed-size ObjectCache
               |                       |
               +-----------+-----------+
                           |
                          Arena
                           |
          +----------------+----------------+
          |                                 |
      kernel VM                          hosted OS
   pages/address ranges              mmap/platform VM
```

### 1. Backing arena

An arena manages ranges of a resource. Memory is the primary use, but the design does not require every resource to be ordinary RAM.

Conceptual interface:

```forge
struct Arena {
    provider: *void;
}
```

A kernel arena may obtain virtual address ranges/pages from Cosmic VM. A hosted arena may use host virtual-memory interfaces. Test code may back an arena with a fixed byte region.

The upper allocation algorithms must not care which provider is used.

### 2. General arbitrary-size allocator

The ordinary allocator interface is the old-school malloc-like path, but explicit and typed by request rather than hidden as a process-global facility.

```forge
struct AllocRequest {
    size: usize;
    align: usize;
    wait: AllocWait;
}

struct MemoryBlock {
    data: *byte;
    size: usize;
    align: usize;
}

struct Allocator {
    arena: &mut Arena;
}
```

Normative operations:

```text
allocator_alloc(allocator, request)
    -> Result[MemoryBlock, AllocError]

allocator_free(allocator, block)
    -> void

allocator_resize(allocator, block, new_size)
    -> Result[MemoryBlock, AllocError]
```

This supports arbitrary byte sizes and alignments, including workloads for which a fixed object cache is inappropriate.

Size and alignment stay explicit so simple allocators do not require hidden boundary tags. Implementations may still maintain internal metadata when their policy benefits from it.

### 3. Fixed-size object cache

Fixed-size objects are a first-class abstraction rather than merely an optimization hidden under general allocation.

```forge
struct ObjectCacheSpec {
    object_size: usize;
    object_align: usize;
    slab_size: usize;
}
```

Normative operations:

```text
object_cache_create(arena, spec)
    -> Result[ObjectCache, AllocError]

object_cache_alloc(cache, wait)
    -> Result[*byte, AllocError]

object_cache_free(cache, object)
    -> void

object_cache_reclaim(cache)
    -> usize

object_cache_stats(cache)
    -> ObjectCacheStats
```

The cache obtains slabs/extents from its arena, divides them into equal-sized aligned objects, and maintains free objects efficiently.

Useful examples include kernel process/thread objects, VFS nodes, packet descriptors, IPC endpoints, filesystem records, compiler AST nodes, database records, ECS component chunks, and fixed-size protocol objects.

It also provides a natural foundation for size-class allocation in a hosted process allocator: a general allocator may internally route common sizes to object caches while keeping the public arbitrary-size API.

## Object-cache geometry

For a valid `ObjectCacheSpec`:

```text
stride = align_up(object_size, object_align)
objects_per_slab = slab_size / stride
```

Initial validity rules:

- `object_size > 0`;
- `object_align` is a non-zero power of two;
- `slab_size` is a non-zero power of two;
- `slab_size >= object_size`;
- aligned stride fits in one slab.

Future implementations may add slab-header/reserved-space accounting without changing caller-facing semantics.

## Constructors and destructors

Object caches should eventually support optional construction/destruction policy, but the initial API should not force function pointers into every cache object.

Likely long-term policy:

```forge
struct ObjectCacheOps {
    constructor: fn(*byte, *void) -> Result[void, AllocError]?;
    destructor: fn(*byte, *void) -> void?;
    reclaim: fn(*void) -> void?;
    context: *void?;
}
```

This lets a kernel keep expensive object initialization cached while retaining lightweight caches for plain fixed-size storage.

## Kernel/user sharing

The design rule is:

> allocation policy and cache algorithms belong in `core`; acquisition of pages/address space and scheduler/locking integration belong to the environment.

```text
core ObjectCache / Allocator algorithms
        |
        +-- Cosmic kernel backing arena
        |      VM pages, kernel locks, per-CPU state
        |
        +-- std hosted backing arena
        |      host VM mappings, user synchronization
        |
        +-- firmware/static arena
               fixed memory region, possibly no synchronization
```

This is the Solaris-like property we want: the same allocator/cache implementation can be compiled into kernel and user mode with different providers.

## Concurrency and magazines

The first implementation should be correct and simple, probably with externally supplied synchronization or a single lock per cache. Per-CPU/thread magazines are a later optimization and must not be part of the fundamental API contract.

```text
per-CPU/thread magazine
        |
central cache depot
        |
slab layer
        |
Arena
```

The API remains unchanged when magazines are added.

## Allocation failure policy

Primitive allocation is fallible:

```forge
Result[T, AllocError]
```

No allocator in `core` silently aborts merely because memory is unavailable. Higher-level applications or kernels may explicitly convert allocation failure into panic when appropriate.

## Blocking policy

Kernel allocation often needs waitable and non-waitable forms. This is explicit:

```forge
enum AllocWait {
    MayWait,
    NoWait,
}
```

A `NoWait` request must never sleep waiting for memory. The environment decides exactly what sleeping/waiting means.

## Reclaim and pressure

Object caches support explicit reclaim so environments can respond to memory pressure:

```text
object_cache_reclaim(cache) -> usize
```

The result is backing storage released in bytes. The environment decides when reclaim is requested; `core` assumes no page daemon or hosted pressure mechanism.

## Statistics and diagnostics

`ObjectCacheStats` begins with counters for allocations, frees, failures, objects in use/free, slab counts, backing bytes, and reclaim calls.

Debug implementations may add poisoning, red zones, duplicate-free detection, ownership checks, and allocation-site metadata without changing normal caller APIs.

## What stays out of `core`

`core` does not provide:

- a hidden process-global allocator;
- OS virtual-memory syscalls;
- kernel page-table manipulation;
- filesystem/network services;
- a mandatory thread implementation;
- hosted process termination policy.

Those belong in `std`, Cosmic, firmware/platform code, or application-specific libraries.