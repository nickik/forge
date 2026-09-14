# Forge `core` library

**Status:** initial v1 design baseline.

`core` is the freestanding standard library shared by hosted programs, kernels, firmware, boot environments, and tests. It contains algorithms and abstractions that do not require an operating system or a default heap.

## Goals

1. Make the same fundamental code usable in kernel and user mode.
2. Keep allocation explicit and fallible.
3. Separate allocation algorithms from the source of address space or physical memory.
4. Treat fixed-size objects as a first-class allocator workload.
5. Permit debugging, accounting, reclaim, and statistics without changing callers.
6. Avoid requiring libc or a process-global allocator.

The allocator architecture is intentionally similar in spirit to the Solaris slab/UMEM family: object caches manage fixed-size objects while a lower backing arena supplies larger extents. The same cache algorithms can therefore be reused with different backing providers in kernel and user environments.

## Initial `core` contents

The first single-file `core.fg` is intentionally small, but the logical API is expected to grow into these areas:

```text
core
  integer/math helpers
  bit manipulation
  memory primitives
  slices and byte spans
  strings/views with no allocation
  Option/Result helpers
  target/layout information
  atomics once language/runtime support is ready
  allocation interfaces
  fixed-size object caches
  arenas/resource allocation
```

Not all items must be implemented in the first bootstrap file.

## Allocation model

Forge does not define `malloc` as a language primitive. Allocation happens through an explicit allocator or an owning object.

The allocator stack is divided into three layers:

```text
                  typed/container users
                         |
                         v
                 ObjectCache / Allocator
                         |
                         v
                 Arena / backing source
                         |
          +--------------+---------------+
          |                              |
      kernel VM                       hosted OS
   pages / address ranges          mmap/brk/platform VM
```

### 1. Backing arena

An arena manages ranges of a resource. Memory is the primary use, but the design should not unnecessarily assume that every resource is ordinary RAM.

Conceptual interface:

```forge
struct Arena {
    // opaque implementation state
}

fn arena_alloc(arena: &mut Arena, size: usize, align: usize)
    -> Result[*byte, AllocError];

fn arena_free(arena: &mut Arena, address: *byte, size: usize) -> void;
```

A kernel arena may obtain virtual address ranges/pages from Cosmic VM. A hosted arena may use the host virtual-memory interface. Test code may back an arena with a fixed byte region.

The upper allocation algorithms must not care which provider is used.

### 2. General allocator

The ordinary allocator interface handles variable-sized byte allocations and is passed explicitly to code that needs durable storage.

Conceptual interface:

```forge
struct Allocator {
    // opaque policy/state
}

fn alloc(
    allocator: &mut Allocator,
    size: usize,
    align: usize
) -> Result[*byte, AllocError];

fn free(
    allocator: &mut Allocator,
    address: *byte,
    size: usize,
    align: usize
) -> void;
```

Size and alignment are explicit. This permits simple implementations, strong checking, and allocators that do not need hidden boundary tags.

A future convenience API may carry allocation metadata in a block value when that is more ergonomic:

```forge
struct MemoryBlock {
    data: *byte;
    size: usize;
    align: usize;
}
```

### 3. Fixed-size object cache

Fixed-size objects are a first-class abstraction rather than merely an optimization hidden under `malloc`.

Conceptual API:

```forge
struct ObjectCache {
    // opaque cache state
}

struct ObjectCacheSpec {
    object_size: usize;
    object_align: usize;
    slab_size: usize;
}

fn object_cache_create(
    arena: &mut Arena,
    spec: ObjectCacheSpec
) -> Result[ObjectCache, AllocError];

fn object_cache_alloc(cache: &mut ObjectCache)
    -> Result[*byte, AllocError];

fn object_cache_free(cache: &mut ObjectCache, object: *byte) -> void;

fn object_cache_reclaim(cache: &mut ObjectCache) -> usize;
```

The cache obtains slabs/extents from its arena, divides them into equal-sized objects, and maintains free objects efficiently.

This is useful directly for:

```text
kernel process/thread objects
VFS nodes
network packet descriptors
IPC endpoints
filesystem records
compiler AST nodes
database records
user-space object pools
fixed-size protocol objects
```

It also provides the natural foundation for size-class allocation in a hosted process allocator.

## Constructors and destructors

Object caches should eventually support optional construction/destruction policy, but the initial API should not force function pointers into every cache object.

The likely long-term shape is a separate policy/configuration record:

```forge
struct ObjectCacheOps {
    constructor: fn(*byte, *void) -> Result[void, AllocError]?;
    destructor: fn(*byte, *void) -> void?;
    reclaim: fn(*void) -> void?;
    context: *void?;
}
```

This allows kernel subsystems to keep initialized object state cached while also allowing lightweight caches for plain fixed-size storage.

Exact optional-function-pointer syntax remains subject to language implementation support.

## Kernel/user sharing

The design rule is:

> allocation policy and cache algorithms belong in `core`; acquisition of pages/address space and scheduler/locking integration belong to the environment.

Example:

```text
core ObjectCache algorithm
        |
        +-- Cosmic kernel backing arena
        |      VM pages, kernel locks, per-CPU state
        |
        +-- std hosted backing arena
               host VM mappings, user-space synchronization
```

This permits the same object-cache implementation to be compiled into both the Cosmic kernel and the hosted Forge runtime.

## Concurrency and magazines

The first implementation should be correct and simple, probably with externally supplied synchronization or a single lock per cache. Per-CPU/thread magazines are a later optimization and must not be part of the fundamental API contract.

The long-term structure may be:

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

Possible initial error categories:

```forge
enum AllocError {
    OutOfMemory,
    InvalidSize,
    InvalidAlignment,
    ResourceLimit,
}
```

## Blocking policy

Kernel allocation often needs both waitable and non-waitable forms. This should not become an implicit global property.

A likely explicit request policy is:

```forge
enum AllocWait {
    MayWait,
    NoWait,
}
```

or a request struct passed to lower-level arena allocation. The ordinary high-level allocator API may default to its allocator's configured policy, while kernel-facing APIs expose the distinction where necessary.

## Reclaim and pressure

Object caches should support explicit reclaim so environments can respond to memory pressure:

```forge
fn object_cache_reclaim(cache: &mut ObjectCache) -> usize;
```

The return value is the amount of backing storage released, in bytes.

The environment decides when reclaim is requested. `core` does not assume a kernel page daemon or hosted memory-pressure mechanism.

## Statistics and diagnostics

Allocator state should be observable from the beginning. Candidate counters include:

```text
allocations
frees
objects_in_use
objects_free
slabs_total
slabs_empty
bytes_backing
allocation_failures
reclaims
```

Debug builds may add poisoning, red zones, duplicate-free detection, ownership checks, and allocation-site metadata without changing the basic allocator API.

## What stays out of `core`

`core` does not provide:

- process-global `malloc/free` policy;
- OS virtual-memory syscalls;
- kernel page-table manipulation;
- filesystem/network services;
- a mandatory thread implementation;
- a hidden default allocator.

Those belong in `std`, Cosmic, firmware/platform code, or an application-specific library.