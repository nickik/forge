# Forge collections library TODO

Forge v1 does not rely on language generics for its standard collections. The collections library therefore uses **generated, type-specialized implementations** built from a small number of audited collection templates.

The goal is not to hand-maintain dozens of nearly identical containers. The goal is:

```text
collection algorithm/template
        |
        +-- element/key/value type description
        +-- equality/hash/ownership policy
        +-- allocator policy
        |
        v
 generated Forge source
        |
        +-- ListU32
        +-- ListString
        +-- HashSetU64
        +-- HashSetString
        +-- HashMapStringString
        +-- HashMapU64U64
        +-- application-specific generated types
```

Generated collections are ordinary Forge modules after generation. There is no special runtime representation and no requirement that the compiler implement generics internally.

## Design principles

- [ ] Collections themselves are implemented in Forge, not supplied by CForge/Java/Unix providers.
- [ ] Use explicit `Allocator`/Arena-backed allocation rather than hiding allocation inside runtime hooks.
- [ ] Allocation failure is returned as `Result[..., AllocError]`; primitive collection operations do not implicitly panic on OOM.
- [ ] Bounds violations remain normal Forge bounds/panic behavior where an indexed operation promises a valid index.
- [ ] Generated collection code must be readable and debuggable Forge source.
- [ ] Generation must be deterministic so generated source can be compared in CI.
- [ ] Algorithms are shared at generation time rather than duplicated manually.
- [ ] Collection layout is type-specialized: no `void*`-style erased element representation for normal typed collections.
- [ ] Keep freestanding algorithms independent of filesystem, process, thread, or other hosted services.
- [ ] Synchronization is external by default. Collections are not implicitly thread-safe.

## Naming and specialization model

Initial generated names should make the concrete type obvious:

```text
ListU8
ListI32
ListU64
ListUsize
ListString

HashSetU32
HashSetU64
HashSetUsize
HashSetString

HashMapStringString
HashMapStringU64
HashMapU64String
HashMapU64U64
```

For user-defined types, generation should derive a stable name from the declared type:

```text
ListProcessId
HashSetPageNumber
HashMapProcessIdProcess
```

- [ ] Freeze generated type/module naming rules.
- [ ] Reject generated-name collisions explicitly.
- [ ] Allow an explicit generated collection name when automatic naming is undesirable.
- [ ] Keep generated public APIs identical across equivalent specializations.

## Collection generation mechanism

Prefer a build-time Forge-aware generator rather than adding a second pseudo-generic language to the compiler.

A package should eventually be able to request concrete collection implementations in `forge.fdn` or a dedicated generation manifest, conceptually:

```text
#forge/collections {
  :list [u8 i32 u64 usize String]

  :hash-set [u32 u64 usize String]

  :hash-map [
    [String String]
    [String u64]
    [u64 String]
    [u64 u64]
  ]
}
```

User types need explicit policies when the defaults are not sufficient, conceptually:

```text
#forge/hash-map {
  :name ProcessTable
  :key ProcessId
  :value Process
  :hash process_id_hash
  :equals process_id_equals
}
```

Exact manifest syntax is not frozen by this TODO.

- [ ] Define collection-generation manifest/schema.
- [ ] Implement generator in the Forge build tooling.
- [ ] Generate into a deterministic build directory.
- [ ] Make generated modules participate in normal dependency/interface checking.
- [ ] Add `forge build --emit-generated` or equivalent debugging facility.
- [ ] CI test that regeneration is deterministic.
- [ ] CI compile generated code with both production Forge and CForge where supported.
- [ ] Do not require generated source to be committed to repositories unless explicitly requested.

## Common collection contracts

### Allocation

- [ ] Every owning dynamic collection stores or receives an allocator capability.
- [ ] Construction returns `Result[Collection, AllocError]` when initial allocation is required.
- [ ] Growth failure leaves the original collection valid and unchanged.
- [ ] `reserve` and `try_reserve` semantics are explicit.
- [ ] Destruction/freeing releases all owned collection storage.
- [ ] Define move/copy behavior for collection values before enabling accidental expensive copies.

### Ownership

- [ ] Define behavior separately for trivially copyable values and owning values such as `String`.
- [ ] Freeze whether insertion moves or copies an owning value.
- [ ] Provide explicit clone/copy operations only where supported.
- [ ] Removal must define whether the removed value is returned to the caller or destroyed.
- [ ] Clearing/destroying must correctly release owned keys/values.

### Iteration

Because Forge does not yet need a generic iterator framework, start with concrete generated iterators.

- [ ] `ListU64Iterator`, `HashSetStringIterator`, etc.
- [ ] Stable basic `next` contract.
- [ ] Mutable iteration only after aliasing rules are clear.
- [ ] Define invalidation rules after collection mutation.
- [ ] No heap allocation merely to create an iterator.

## `List<T>` family

`List` should initially mean a **contiguous growable vector**, not a linked list. This is the default general-purpose sequence container and is more useful for Forge/Cosmic than a heap-node-per-element linked list.

Initial specializations:

- [ ] `ListU8` — byte buffers and binary protocols.
- [ ] `ListI32`.
- [ ] `ListU32`.
- [ ] `ListI64`.
- [ ] `ListU64`.
- [ ] `ListUsize`.
- [ ] `ListString`.
- [ ] Generator support for application-defined element types.

Required operations:

- [ ] `create(allocator)` / `with_capacity(allocator, capacity)`.
- [ ] `len`.
- [ ] `capacity`.
- [ ] `is_empty`.
- [ ] indexed `get`.
- [ ] indexed mutable access when Forge aliasing support is ready.
- [ ] `push`.
- [ ] `pop`.
- [ ] `insert`.
- [ ] `remove`.
- [ ] `swap_remove`.
- [ ] `clear`.
- [ ] `reserve` / capacity growth.
- [ ] `truncate`.
- [ ] slice/view of contiguous contents.
- [ ] iterator.
- [ ] destroy/free.

Implementation/testing:

- [ ] Geometric capacity growth with overflow checks.
- [ ] Zero-capacity construction.
- [ ] Growth from 0/1/small capacities.
- [ ] Preserve contents across realloc/growth.
- [ ] Remove from start/middle/end.
- [ ] Allocation-failure injection.
- [ ] Capacity arithmetic overflow tests.
- [ ] Large list tests.
- [ ] Owning `String` destruction tests.

## `HashSet<T>` family

Initial specializations:

- [ ] `HashSetU32`.
- [ ] `HashSetU64`.
- [ ] `HashSetUsize`.
- [ ] `HashSetString`.
- [ ] Generated application-specific set types.

Required operations:

- [ ] `create(allocator)` / `with_capacity`.
- [ ] `len`.
- [ ] `is_empty`.
- [ ] `contains`.
- [ ] `insert` returning whether the value was newly inserted.
- [ ] `remove` returning whether the value existed.
- [ ] `clear`.
- [ ] `reserve`.
- [ ] iterator.
- [ ] destroy/free.

Hash-table implementation:

- [ ] Choose and document initial probing strategy; open addressing is preferred for the first implementation.
- [ ] Define empty/occupied/deleted slot representation without requiring nullable values.
- [ ] Define maximum load factor and growth threshold.
- [ ] Rehash correctly after growth.
- [ ] Tombstone handling/removal.
- [ ] Avoid pathological infinite probe loops when table contains tombstones.
- [ ] Deterministic behavior for deterministic hashes; iteration order itself is not guaranteed API.
- [ ] Collision stress tests.
- [ ] Full-table/tombstone stress tests.
- [ ] Repeated grow/remove/grow tests.
- [ ] Allocation-failure tests during rehash.

## `HashMap<K,V>` family

HashMap uses the same hash-table engine as HashSet; HashSet should effectively be a generated specialization of the same probing machinery rather than a separately invented algorithm.

Initial standard specializations needed soon:

- [ ] `HashMapStringString` — replace CKV bootstrap `StringMap`.
- [ ] `HashMapStringU64` — names/counters/IDs.
- [ ] `HashMapStringUsize` — indexes and lookup tables.
- [ ] `HashMapU32U32`.
- [ ] `HashMapU64U64`.
- [ ] `HashMapU64Usize`.
- [ ] `HashMapUsizeUsize`.
- [ ] `HashMapU64String` where owning values are useful.
- [ ] Generated application-specific key/value combinations.

Required operations:

- [ ] `create(allocator)` / `with_capacity`.
- [ ] `len`.
- [ ] `is_empty`.
- [ ] `contains_key`.
- [ ] `get`.
- [ ] mutable lookup after aliasing rules are ready.
- [ ] `put`/`insert`, with explicit replace semantics.
- [ ] `remove`.
- [ ] `clear`.
- [ ] `reserve`.
- [ ] key iterator.
- [ ] value iterator.
- [ ] entry iterator.
- [ ] destroy/free.

Replacement/removal contracts:

- [ ] Freeze whether `put` returns old value, replacement flag, or a small generated result type.
- [ ] Freeze whether `remove` returns removed value/Option-like result.
- [ ] Ensure replacing owning `String` keys/values releases or returns old storage exactly once.
- [ ] Verify updating an existing key does not grow `len`.

## Hashing library

Collections need hashing independent of the map implementation.

- [ ] Add a small freestanding hashing module (`core.hash` or equivalent).
- [ ] Hash `u8/u16/u32/u64/usize`.
- [ ] Hash byte slices.
- [ ] Hash UTF-8 string bytes.
- [ ] Select a simple stable non-cryptographic default hash suitable for tables.
- [ ] Do not promise hash values as persistent/on-disk ABI unless explicitly versioned.
- [ ] Allow application-generated maps to supply custom hash/equality functions.
- [ ] Keep cryptographic hashing separate from collection hashing.

## Equality requirements

- [ ] Primitive numeric specializations use normal value equality.
- [ ] String specialization uses exact string/byte equality according to Forge String semantics.
- [ ] Application-defined key types must supply equality unless the language can generate a correct structural equality operation.
- [ ] Hash/equality contract test: equal keys must hash equivalently.

## Optional later collection families

These are useful but should not block List/HashSet/HashMap.

### `Deque<T>` / ring deque

- [ ] Generate for common primitive and user-defined types.
- [ ] Push/pop at both ends.
- [ ] Circular buffer implementation.
- [ ] Useful for queues, work lists and userspace services.

### `LinkedList<T>`

Do not make this the default `List`. Add only where stable-node-address semantics are specifically useful.

- [ ] Singly/doubly linked form decision.
- [ ] Intrusive form should generally remain in `core.intrusive` for kernels.
- [ ] Owning heap-node linked list only if concrete use cases justify it.

### `BitSet`

- [ ] Fixed/dynamic bit-set variants.
- [ ] Efficient set/clear/test and scans.
- [ ] Useful for allocation maps, CPU masks, descriptor tables and protocol state.

### Ordered structures

- [ ] `TreeMap`/`TreeSet` only after a use case justifies ordering/range lookup.
- [ ] Prefer sorted `List` + binary search for small mostly-static tables.

### Priority queue

- [ ] Binary heap specialization if schedulers/search algorithms need it.
- [ ] Avoid implementing solely for completeness.

## Initial implementation order

### Phase 1 — infrastructure

- [ ] Freeze generated naming rules.
- [ ] Freeze allocator/ownership contracts.
- [ ] Implement deterministic collection source generator.
- [ ] Implement common typed storage helpers needed by generated collections.
- [ ] Add freestanding default hashing/equality helpers.

### Phase 2 — List first

- [ ] `ListU8`.
- [ ] `ListU64`.
- [ ] `ListUsize`.
- [ ] `ListString`.
- [ ] General generated user-type List.
- [ ] Full growth/OOM/destruction test matrix.

This provides the dynamic-buffer foundation needed by strings, files, protocol parsers and later hash-table internals.

### Phase 3 — hash-table engine

- [ ] Build one generated open-addressing table engine.
- [ ] Implement `HashSetU64` and `HashSetString` first.
- [ ] Implement `HashMapStringString`.
- [ ] Replace CKV's bootstrap `std.collections.string_map` provider with Forge `HashMapStringString`.
- [ ] Run CKV unchanged on the Forge map implementation.
- [ ] Add numeric map specializations.
- [ ] Add custom user-type key/value generation.

### Phase 4 — hardening

- [ ] Collision-heavy tests with deliberately poor/custom hash function.
- [ ] Allocation-failure injection at every growth/rehash allocation point.
- [ ] Fuzz/model tests against a simple reference implementation.
- [ ] Long random operation sequences: insert/get/remove/clear/reserve.
- [ ] Validate no leaks/double frees with owning String values.
- [ ] Benchmark common specializations.
- [ ] Compare generated code size and runtime performance with equivalent C/Rust containers where useful.

### Phase 5 — Cosmic use cases

- [ ] Generate process/thread/task tables keyed by strongly typed IDs.
- [ ] Generate VM/page metadata lookup tables where hashing is appropriate.
- [ ] Use `ListU8`/typed Lists in protocol and IPC buffers where dynamic ownership is acceptable.
- [ ] Keep allocation-free/fixed-capacity kernel hot paths on `core.fixed` or `core.intrusive` instead of forcing dynamic collections everywhere.
- [ ] Verify same portable collection implementation works with hosted and Cosmic allocators.

## CKV acceptance gate

The first major acceptance criterion for the collection library is removal of the CForge-provided map implementation from CKV:

```text
CKV Forge source
      |
      v
HashMapStringString   <-- Forge generated implementation
      |
      v
core/std allocator
      |
      +-- hosted provider
      +-- Cosmic provider
```

- [ ] CKV no longer calls a host-provided `StringMap` implementation.
- [ ] CKV persistence tests remain unchanged and green.
- [ ] JVM CForge execution uses the Forge map code.
- [ ] GraalVM/native CForge execution uses the same Forge map code.
- [ ] Production Forge compiler eventually executes the same source.
- [ ] Cosmic userspace eventually executes the same source.

## Explicitly deferred

Do not block the first collection implementation on:

- language generics;
- generic traits/typeclasses;
- universal iterator abstractions;
- concurrent hash maps;
- lock-free containers;
- persistent/immutable functional collections;
- B-trees/red-black trees without a concrete use case;
- full Unicode-aware collection semantics;
- serialization of arbitrary collection types.

The critical path is **generated typed List + shared hash-table engine + generated HashSet/HashMap**, with `HashMapStringString` replacing CKV's bootstrap host map as the first end-to-end proof.