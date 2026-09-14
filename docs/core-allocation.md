# Forge `core` Allocation Architecture

**Status:** v1 design baseline for the standard library.

## 1. Two first-class allocation workloads

Forge `core` supports both:

1. **variable-sized allocation** — the traditional malloc-like workload;
2. **fixed-size object allocation** — repeated allocation of one known object shape.

Neither is a language primitive and neither requires a process-global heap.

## 2. Provider capability boundary

Forge does not require traits/interfaces for allocator polymorphism. The provider boundary is explicit data:

```forge
pub struct ArenaOps {
    alloc: fn(*void, AllocRequest) -> Result[MemoryBlock, AllocError];
    free: fn(*void, MemoryBlock) -> void;
    reclaim: fn(*void, usize) -> usize;
}

pub struct Arena {
    context: *void;
    ops: &ArenaOps;
}
```

Dispatch is ordinary Forge code:

```forge
return arena.ops.alloc(arena.context, request);
```

This is deliberately transparent: no hidden vtable, RTTI, object header, allocation, or inheritance is involved.

The same mechanism permits different providers:

```text
Arena
  +-- CosmicPageArena      kernel VM/pages
  +-- HostedVmArena        hosted platform VM
  +-- FixedArena           static RAM/test buffer
```

The algorithms above the capability boundary must not depend on which provider is selected.

## 3. Variable-sized Allocator

The general allocator is also an explicit capability object:

```forge
pub struct AllocatorOps {
    alloc: fn(*void, AllocRequest) -> Result[MemoryBlock, AllocError];
    free: fn(*void, MemoryBlock) -> void;
    resize: fn(*void, MemoryBlock, usize) -> Result[MemoryBlock, AllocError];
}

pub struct Allocator {
    context: *void;
    ops: &AllocatorOps;
}
```

The public wrapper supports arbitrary sizes and alignments while keeping allocation fallible.

A common concrete implementation may be:

```text
small requests -> size-class ObjectCaches
large requests -> direct Arena extents
```

but this is allocator policy, not part of the `Allocator` ABI.

## 4. Fixed-size ObjectCache

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
ObjectCache
  +-- free objects
  +-- in-use objects
  +-- partial/full/empty slabs
          |
          v
        Arena
```

Geometry:

```text
stride = align_up(object_size, object_align)
objects_per_slab = slab_size / stride
```

When the free set is empty, the cache requests a new slab from its `Arena` using the caller's wait policy. Completely unused slabs may later be returned to the `Arena` during explicit reclaim.

## 5. Wait policy

```forge
pub enum AllocWait {
    MayWait,
    NoWait,
}
```

`NoWait` is semantically significant. Provider dispatch must preserve it exactly. Kernel interrupt/scheduler/VM paths may rely on the guarantee that a `NoWait` request never sleeps waiting for memory.

## 6. Failure policy

Allocation failures are values:

```text
Result[MemoryBlock, AllocError]
```

Primitive allocation never automatically invokes panic. Kernel code can therefore retry, reclaim, use emergency reserves, degrade service, return an error, or explicitly panic.

## 7. Kernel/user reuse

The essential design requirement is not merely source compatibility: the **same cache algorithm** must run over both kernel-like and hosted-like providers.

Reference tests therefore instantiate equivalent provider capabilities with different identities and run the same sequences against each:

```text
allocate arbitrary blocks
allocate enough fixed objects to grow slabs
verify live-object uniqueness
free/reuse objects
reclaim completely empty slabs
verify live slabs survive reclaim
propagate NoWait
observe provider reclaim calls
```

Any semantic difference caused only by provider identity is a bug in `core`.

## 8. Deterministic reference model

Until the Forge bootstrap can execute all required raw-pointer mutation and private allocator state, CForge contains an executable semantic model of:

```text
Arena capability dispatch
Allocator capability dispatch
ObjectCache slab growth/free/reuse/reclaim
```

The model represents addresses as integer tokens and deliberately contains no host VM dependency. This lets CI test kernel/user provider independence now.

Once the language implementations support the complete Forge allocator source, the same behavioral fixtures must be run against the real `core.fg` implementation and then against bare-metal/QEMU providers.

## 9. Concurrency

The initial contract contains no hidden lock. Correct single-threaded/provider semantics come first.

Later scalable implementations may introduce:

```text
per-CPU/per-thread magazine
        -> central depot
        -> slab/cache
        -> Arena
```

Provider/environment synchronization policy must remain outside the basic allocation ABI.

## 10. ECS relationship

The allocator is **data-oriented and ECS-compatible**, not an ECS itself. Homogeneous storage, predictable object sizes and bulk lifecycle are common ideas; entity identity, component composition, archetypes, queries and systems are intentionally absent.

## 11. Test gates before Cosmic kernel work

Before Cosmic relies on this layer, the following must be green:

- provider dispatch is explicit and source-contract tested;
- kernel-like and hosted-like providers pass identical behavioral tests;
- arbitrary-size allocation works across varied sizes/alignment;
- OOM is returned, not panicked;
- fixed-cache objects are unique while live;
- free/reuse works;
- multiple slabs grow correctly;
- reclaim releases only empty slabs;
- invalid/double free is detected by the reference/debug model;
- `NoWait` reaches the provider unchanged;
- statistics remain internally consistent;
- freestanding panic needs neither allocator nor unwinder.

The later bare-metal integration gate adds QEMU tests with a real Cosmic page provider and a hosted VM provider using the same `core` cache implementation.
