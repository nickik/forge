# Forge `core` Allocation Architecture

**Status:** v1 design baseline for the standard library.

## 1. Two first-class allocation workloads

Forge `core` supports both:

1. **variable-sized allocation** — the traditional malloc-like workload;
2. **fixed-size object allocation** — repeated allocation of one known object shape.

Neither is a language primitive and neither requires a process-global heap.

The public allocation discipline is explicit: ordinary values do not retain allocator capabilities merely so future operations can allocate. A caller passes the relevant allocator to each operation that allocates, resizes, or frees durable memory.

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

`Arena`, `Allocator`, `ObjectCache`, slab allocators, and explicit pool/memory-domain managers are themselves memory-management objects. They may retain lower-level provider capability state because implementing a memory domain is their purpose. This is distinct from an ordinary collection, string, buffer, parser result, or application value retaining an allocator for later convenience.

## 3. Variable-sized Allocator

The general allocator is an explicit capability object:

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

### 3.1 Call-site allocation rule

The allocator is supplied to the operation that actually needs it:

```forge
list_u8_push(&mut bytes, &mut allocator, value)?;
list_u8_try_reserve(&mut bytes, &mut allocator, 4096)?;
list_u8_destroy(&mut bytes, &mut allocator);
```

The collection contains its own backing block and metadata but not the allocator capability:

```forge
pub struct ListU8 {
    block: MemoryBlock?;
    len: usize;
    capacity: usize;
}
```

Operations that cannot allocate or free do not take the allocator.

There is no fallback to a process-global allocator, thread-local allocator, `context.scratch`, libc heap, or an allocator remembered during construction.

### 3.2 Allocation provenance

A `MemoryBlock` remains associated with the allocator domain that produced it even though the owning collection does not store that allocator capability.

The caller is responsible for supplying the same allocator domain for later `resize` and `free`. A different allocator instance may be used only when its provider explicitly documents that it belongs to the same compatible allocation domain and accepts those blocks.

This contract has several consequences:

- moving a collection transfers the obligation to use a compatible allocator for its backing blocks;
- swapping collection values does not change block provenance;
- a collection may receive different compatible allocator handles across operations, but incompatible allocator domains are invalid;
- debug/reference providers should detect wrong-domain resize/free when practical;
- allocation failure during growth must leave the original block owned by its original domain and the collection valid.

The collection deliberately does not spend a word retaining allocator identity. Provenance is an API/lifetime responsibility, with optional debug validation by allocator implementations.

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

When the free set is empty, the cache requests a new slab from its retained backing `Arena` using the caller's wait policy. Completely unused slabs may later be returned to the `Arena` during explicit reclaim.

`ObjectCache` is an explicit memory-domain manager, so retaining its backing Arena capability is intentional and does not weaken the ordinary-value explicit-allocator rule.

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

The essential design requirement is not merely source compatibility: the **same allocation and collection algorithms** must run over both kernel-like and hosted-like providers.

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

Ordinary library call sites still pass allocators explicitly in both environments; hosted execution does not gain an implicit allocator shortcut.

## 8. Deterministic reference model

Until the Forge bootstrap can execute all required raw-pointer mutation and private allocator state, CForge contains an executable semantic model of:

```text
Arena capability dispatch
Allocator capability dispatch
ObjectCache slab growth/free/reuse/reclaim
```

The model represents addresses as integer tokens and deliberately contains no host VM dependency. This lets CI test kernel/user provider independence now.

Once the language implementations support the complete Forge allocator source, the same behavioral fixtures must be run against the real `core.fg` implementation and then against bare-metal/QEMU providers.

CForge bootstrap raw-storage helpers must not define a different public ownership model for collections. They are implementation scaffolding only.

## 9. Concurrency

The initial contract contains no hidden lock. Correct single-threaded/provider semantics come first.

Later scalable allocator implementations may introduce:

```text
per-CPU/per-thread magazine
        -> central depot
        -> slab/cache
        -> Arena
```

That remains internal allocator policy. It does not create a thread-local/default allocator visible to ordinary allocating APIs.

Provider/environment synchronization policy must remain outside the basic allocation ABI.

## 10. ECS relationship

The allocator is **data-oriented and ECS-compatible**, not an ECS itself. Homogeneous storage, predictable object sizes and bulk lifecycle are common ideas; entity identity, component composition, archetypes, queries and systems are intentionally absent.

An ECS `World` may be an explicit memory-domain manager if that is part of its defined architecture, but ordinary collection fields within it still do not implicitly acquire a global allocator policy. APIs that operate on ordinary collections follow the explicit allocator contract.

## 11. Test gates before Cosmic kernel work

Before Cosmic relies on this layer, the following must be green:

- provider dispatch is explicit and source-contract tested;
- ordinary collections/owning values contain no retained allocator capability;
- every durable allocate/grow/free API exposes allocator use in its signature;
- wrong-domain resize/free is detected by reference/debug providers where practical;
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
