# Forge `core` Allocation Architecture

**Status:** v1 design baseline for the standard library.

This document refines the allocation section of `core-library.md`.

## 1. Two first-class allocation workloads

Forge `core` supports both:

1. **variable-sized allocation** — the traditional malloc-like workload;
2. **fixed-size object allocation** — repeated allocation of objects of one known size/alignment.

Neither is treated as a language primitive and neither requires a process-global heap.

## 2. Variable-sized allocator

The portable allocator contract is explicit and fallible:

```forge
fn alloc(allocator: &mut Allocator, request: AllocRequest)
    -> Result[MemoryBlock, AllocError];

fn free(allocator: &mut Allocator, block: MemoryBlock) -> void;

fn realloc(
    allocator: &mut Allocator,
    block: MemoryBlock,
    new_size: usize
) -> Result[MemoryBlock, AllocError];
```

`MemoryBlock` carries the address, size, and alignment. This permits malloc-like arbitrary sizes while avoiding a language requirement for hidden boundary tags.

A hosted compatibility library may trivially wrap this with C-shaped `malloc/free/realloc` interfaces when interoperating with C. That compatibility surface is not the fundamental Forge allocation API.

## 3. Fixed-size object cache

`ObjectCache` serves one object shape repeatedly:

```forge
pub struct ObjectCacheSpec {
    object_size: usize;
    object_align: usize;
    slab_size: usize;
}
```

Conceptually:

```text
ObjectCache<T-like shape>
        |
        +-- free objects
        +-- partial slabs
        +-- full slabs
        +-- empty slabs
        |
        v
      Arena
```

The cache can preserve constructor-initialized state, add debugging/redzones, gather statistics, and release empty slabs under pressure.

## 4. Relationship between the two

These abstractions compose rather than compete.

A general allocator may use object caches as size classes:

```text
alloc(24 bytes)  -> cache-32
alloc(70 bytes)  -> cache-96/128
alloc(4 KiB)     -> page/large-allocation path
alloc(2 MiB)     -> arena/VM path
```

Subsystems that naturally allocate one object type can bypass the general allocator and use their `ObjectCache` directly.

This is analogous to the useful separation in Solaris between object caches and the lower backing resource allocator: the same fixed-size allocator machinery can serve kernel objects and user-space allocations while the environment supplies different backing memory.

## 5. Arena/provider boundary

`Arena` is the lower resource provider. It supplies aligned extents and accepts released extents.

The `core` algorithms do not know whether the backing resource came from:

- Cosmic kernel VM/pages;
- a hosted `mmap`-like service;
- firmware RAM;
- a statically reserved region;
- a deterministic test buffer.

This is the central kernel/user sharing boundary.

## 6. Waiting policy

Allocation requests may carry:

```forge
pub enum AllocWait {
    MayWait,
    NoWait,
}
```

`NoWait` is important for interrupt paths, scheduler/VM internals, and embedded code. The provider must not silently block a `NoWait` request.

A hosted allocator can usually use `MayWait` by default at its API boundary, while kernel subsystems can state the policy explicitly.

## 7. Allocation failure

All primitive allocation is fallible. `OutOfMemory`, resource limits, invalid sizes, and invalid alignment are values, not implicit panics.

An explicit convenience layer may provide `alloc_or_panic`, but it is defined in terms of ordinary `alloc` plus explicit panic conversion. This keeps kernels free to reclaim, retry, degrade service, or terminate only the affected subsystem.

## 8. Reclaim

Object caches expose explicit reclaim. A reclaim pass should preferentially return completely unused slabs/extents to the backing arena.

The environment decides *when* to reclaim. `core` contains the mechanism, not page-daemon or process-pressure policy.

## 9. Concurrency

The first implementation favors correctness over scalability. Synchronization is supplied explicitly by the environment or wrapper.

Later optimizations may add:

```text
per-CPU/per-thread magazine
        -> depot
        -> slab/cache
        -> arena
```

This must not change the public object-cache contract.

## 10. ECS relationship

The design is **data-oriented and ECS-compatible**, but `ObjectCache` is not itself an ECS.

The shared ideas are:

- homogeneous fixed-size storage;
- dense/type-segregated memory;
- predictable layout and allocation cost;
- bulk lifecycle/reclaim opportunities;
- reduced per-object allocator metadata.

An ECS adds semantics that the allocator intentionally does not: entity identity, component membership, archetype/query logic, iteration rules, and system scheduling.

An ECS implementation can therefore build component pools or archetype chunks on top of `Arena`/`ObjectCache`, but the allocator stays generally useful to kernels, compilers, databases, network stacks, and ordinary applications.

## 11. Required tests

Eventually executable allocation tests must cover:

- arbitrary-size allocation across small/medium/large sizes;
- alignment from 1 through target page/cache-line alignments;
- zero/invalid size policy;
- invalid non-power-of-two alignment;
- allocation failure with no panic;
- realloc grow/shrink/preserve-data behavior;
- free/reuse;
- fixed-size cache object uniqueness;
- slab growth and empty-slab reclaim;
- `NoWait` propagation;
- constructor/destructor behavior when introduced;
- statistics invariants;
- double-free/corruption detection in debug mode;
- identical cache algorithm tests over kernel-like and hosted-like fake arenas.

Until pointer/store execution exists, the frontend CI tests the shipped `core` source contract and conformance fixtures reserve these behaviors.