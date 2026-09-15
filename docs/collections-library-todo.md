# Forge collections library TODO

Forge v1 does not rely on language generics for standard collections. The collections library therefore uses **generated, type-specialized Forge implementations** built from a small number of audited templates.

The central allocation rule is normative:

> **Collections never retain an `Allocator` capability. Any operation that may allocate, reallocate, grow, clone owning storage, or free durable storage receives an explicit allocator argument at the call site.**

This is intentionally stricter than a generic “store or receive allocator” model.

## Architecture

```text
collection algorithm/template
        |
        +-- element/key/value type description
        +-- equality/hash/ownership policy
        |
        v
generated Forge source
        |
        +-- ListU8
        +-- ListU64
        +-- ListString
        +-- HashSetU64
        +-- HashSetString
        +-- HashMapStringString
        +-- HashMapStringU64
        +-- HashMapU64U64
        +-- application-specific generated types
```

Generated collections are ordinary Forge modules. There is no special runtime representation and no requirement that the language implement user generics.

## Design principles

- [x] Use generated concrete types instead of erased `void*` containers.
- [x] Keep collection policy in Forge source rather than CForge/Java/runtime collection objects.
- [x] Use explicit `Allocator` arguments for durable allocation.
- [x] **Do not store `Allocator`, `&Allocator`, or `&mut Allocator` in ordinary collection values.**
- [x] Operations that cannot allocate/free do not take an allocator.
- [x] Allocation failure is returned as `Result[..., AllocError]`.
- [ ] Growth failure leaves the original collection valid and unchanged.
- [ ] Bounds violations use normal Forge bounds/panic semantics.
- [x] Generation is deterministic and checked in CI.
- [ ] Generated source is compiled by the production Forge frontend.
- [ ] Synchronization remains external; collections are not implicitly thread-safe.
- [ ] Kernel and userspace use the same collection algorithms with different explicit allocator providers.

## Explicit allocator contract

### Required collection layout

A dynamic collection owns its storage block(s), length/capacity/table metadata, but **not the allocator used to obtain those blocks**.

Required shape:

```forge
pub struct ListU8 {
    block: core.MemoryBlock?;
    len: usize;
    capacity: usize;
}

pub struct HashMapU64U64 {
    states: core.MemoryBlock?;
    keys: core.MemoryBlock?;
    values: core.MemoryBlock?;
    len: usize;
    buckets: usize;
    tombstones: usize;
}
```

Forbidden shape:

```forge
pub struct ListU8 {
    allocator: &mut core.Allocator; // forbidden
    ...
}
```

This rule applies to:

- `List*`
- dynamic owning `String`
- `HashSet*`
- `HashMap*`
- `Deque*`
- dynamic `BitSet`
- trees
- priority queues
- future generated ordinary containers

Specialized memory-domain objects such as `Allocator`, `Arena`, slab allocators, object caches, and explicit pool managers are not ordinary collections and may retain their backing-provider capabilities as required to implement the memory domain.

### Call-site rule

Operations that may allocate/reallocate/free receive an allocator:

```forge
list_u8_with_capacity(&mut allocator, capacity)
list_u8_push(&mut list, &mut allocator, value)
list_u8_insert(&mut list, &mut allocator, index, value)
list_u8_try_reserve(&mut list, &mut allocator, capacity)
list_u8_destroy(&mut list, &mut allocator)

hash_set_u64_insert(&mut set, &mut allocator, value)
hash_set_u64_try_reserve(&mut set, &mut allocator, capacity)
hash_set_u64_destroy(&mut set, &mut allocator)

hash_map_string_u64_put(&mut map, &mut allocator, key, value)
hash_map_string_u64_try_reserve(&mut map, &mut allocator, capacity)
hash_map_string_u64_destroy(&mut map, &mut allocator)
```

Operations that cannot allocate/free do not receive one:

```forge
list_u8_len(&list)
list_u8_get(&list, index)
list_u8_pop(&mut list)
list_u8_remove(&mut list, index)

hash_set_u64_contains(&set, value)
hash_set_u64_remove(&mut set, value)

hash_map_string_u64_contains(&map, key)
hash_map_string_u64_get(&map, key)
hash_map_string_u64_remove(&mut map, key)
```

If removal/clear of an owning element type must release nested owned memory, that operation becomes allocator-taking. The generated ownership policy must make this explicit rather than hiding it.

### Empty construction

Zero-capacity construction should not allocate:

```forge
list_u8_create() -> ListU8
hash_set_u64_create() -> HashSetU64
hash_map_u64_u64_create() -> HashMapU64U64
```

The first growing operation receives an allocator and obtains backing storage.

This requires optional backing blocks for zero-capacity values, e.g. `MemoryBlock?`.

### No hidden fallback

Collection code must not silently use:

- process-global allocator
- thread-local allocator
- `context.scratch`
- libc `malloc`
- host/JVM heap as the semantic allocator
- an allocator remembered from construction

CForge may emulate raw `MemoryBlock` storage during bootstrap, but the Forge collection API and layout must remain identical to the native design.

## Naming and specialization model

Initial names:

```text
ListU8
ListI32
ListU32
ListI64
ListU64
ListUsize
ListString

HashSetU32
HashSetU64
HashSetUsize
HashSetString

HashMapStringString
HashMapStringU64
HashMapStringUsize
HashMapU64String
HashMapU64U64
HashMapU64Usize
HashMapUsizeUsize
```

For user-defined types:

```text
ListProcessId
HashSetPageNumber
HashMapProcessIdProcess
```

- [ ] Freeze generated type/module naming rules.
- [ ] Reject generated-name collisions.
- [ ] Allow explicit generated names.
- [ ] Keep equivalent specialization APIs structurally identical.

## Collection generation

The current Rust generator is a bootstrap build tool. Longer term a package should request concrete collections declaratively.

Conceptually:

```text
#forge/collections {
  :list [u8 i32 u64 usize String]
  :hash-set [u32 u64 usize String]
  :hash-map [
    [String String]
    [String u64]
    [u64 u64]
  ]
}
```

Application key types may supply hash/equality policy.

- [x] Deterministic generator exists.
- [x] Requested initial specializations are emitted.
- [x] CI compares generated native output byte-for-byte.
- [x] Generator has a regression test forbidding stored allocator fields.
- [ ] Define generation manifest/schema.
- [ ] Make generated modules participate fully in dependency/interface checking.
- [ ] Add `forge build --emit-generated` or equivalent.
- [ ] Generate application-defined specializations.

## `List<T>` family

`List` means a contiguous growable vector, not a linked list.

Initial specializations:

- [x] `ListU8`
- [x] `ListU64`
- [x] `ListUsize`
- [x] `ListString`
- [ ] `ListI32`
- [ ] `ListU32`
- [ ] `ListI64`
- [ ] user-defined generated element types

Required operations:

- [x] zero-capacity `create()`
- [x] allocator-taking `with_capacity`
- [x] `len`
- [x] `capacity`
- [x] `is_empty`
- [ ] real Forge indexed `get`
- [ ] real Forge indexed `set`
- [ ] allocator-taking `push`
- [ ] `pop`
- [ ] allocator-taking `insert`
- [ ] `remove`
- [ ] `swap_remove`
- [ ] `clear`
- [ ] allocator-taking `reserve`
- [ ] `truncate`
- [ ] slice/view
- [ ] iterator
- [ ] allocator-taking `destroy`

Tests:

- [ ] zero-capacity construction
- [ ] growth from zero
- [ ] geometric capacity growth
- [ ] capacity overflow
- [ ] content preservation after resize
- [ ] allocation failure at every growth point
- [ ] remove start/middle/end
- [ ] owning-string destruction

## Hash-table engine

HashSet and HashMap use one generated open-addressing design:

- linear probing
- explicit empty/occupied/tombstone state
- bounded load factor
- growth/rehash
- deterministic hash/equality semantics
- no nullable-key sentinel tricks

- [x] `core.hash` freestanding hashing module started.
- [ ] Complete byte/string/numeric hashing surface.
- [ ] Build probing/rehash operations in generated Forge rather than native collection hooks.
- [ ] Ensure rehash allocation failure leaves old table intact.
- [ ] Tombstone stress tests.
- [ ] Deliberate collision tests.
- [ ] long random model tests.

## `HashSet<T>` family

Initial:

- [x] `HashSetU64`
- [x] `HashSetString`
- [ ] `HashSetU32`
- [ ] `HashSetUsize`
- [ ] application-defined generated sets

Required API:

- [x] zero-capacity `create()`
- [ ] `len`
- [ ] `is_empty`
- [ ] `contains`
- [ ] allocator-taking `insert`
- [ ] `remove`
- [ ] `clear`
- [ ] allocator-taking `try_reserve`
- [ ] iterator
- [ ] allocator-taking `destroy`

## `HashMap<K,V>` family

Initial:

- [x] `HashMapStringString`
- [x] `HashMapStringU64`
- [x] `HashMapStringUsize`
- [x] `HashMapU64U64`
- [ ] `HashMapU32U32`
- [ ] `HashMapU64Usize`
- [ ] `HashMapUsizeUsize`
- [ ] `HashMapU64String`
- [ ] application-defined generated maps

Required API:

- [x] zero-capacity `create()`
- [ ] `len`
- [ ] `is_empty`
- [ ] `contains_key`
- [ ] `get`
- [ ] allocator-taking `put`
- [ ] `remove`
- [ ] `clear`
- [ ] allocator-taking `try_reserve`
- [ ] key/value/entry iterators
- [ ] allocator-taking `destroy`

Replacement/removal ownership rules must be explicit for owning keys/values.

## CKV acceptance gate

CKV is the first application-level collection proof:

```text
CKV Forge source
      |
      v
HashMapStringString
      |
      v
explicit allocator parameter
      |
      +-- hosted provider
      +-- Cosmic provider
```

- [x] CKV database algorithm is Forge.
- [x] CKV no longer depends on the original host `StringMap` API for its canonical algorithm.
- [ ] CKV moves to the final explicit-allocator `HashMapStringString` API.
- [ ] same tests pass on CForge JVM.
- [ ] same tests pass on CForge native image.
- [ ] same source passes production Forge native backend.
- [ ] same source eventually runs in Cosmic userspace.

## Future families

Only add with a concrete use case:

- `Deque<T>`
- dynamic `BitSet`
- priority queue/binary heap
- ordered map/set
- owning linked list where stable node addresses matter

Intrusive kernel lists remain in `core.intrusive`. Fixed-capacity hot-path structures remain in `core.fixed`.

## Explicitly deferred

Do not block List/HashSet/HashMap on:

- language generics
- generic traits/typeclasses
- universal iterator abstractions
- concurrent maps
- lock-free containers
- persistent functional collections
- B-trees/red-black trees without need
- full Unicode collection semantics
- arbitrary serialization

The critical path is **generated typed collections whose ordinary values contain only their own storage/metadata and whose allocation behavior is explicit in every allocating/freeing call signature**.
