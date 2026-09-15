# Forge v1 Memory and Allocation Model

This document specifies standard conventions and library architecture around the language's explicit memory model.

## Principles

1. No mandatory garbage collector.
2. No hidden allocation for ordinary value operations.
3. Every durable allocate/reallocate/grow/clone/free operation receives an explicit allocator argument at that call site.
4. Ordinary values and collections do not retain allocator capabilities merely to allocate later.
5. Temporary non-escaping allocation may use `context.scratch`; it is never a durable-allocation fallback.
6. Primitive allocation is fallible.
7. Wrong-allocator operations should be detected where reasonably possible rather than becoming silent corruption.
8. Large systems should organize memory by lifetime and data domain while still making the allocator used for each durable allocation operation explicit.

## Allocator interface

Conceptual concrete v1 interface:

```forge
struct Allocator {
    state: *void;
    ops: *AllocatorOps;
}

struct AllocatorOps {
    alloc: fn(state: *void, size: usize, align: usize)
        -> Result[*void, AllocError];
    resize: fn(state: *void, ptr: *void, old_size: usize,
               new_size: usize, align: usize)
        -> Result[*void, AllocError];
    free: fn(state: *void, ptr: *void, size: usize, align: usize)
        -> Result[void, AllocError];
}
```

`Allocator` is itself a memory-management capability and therefore contains its provider state. That does not imply that values allocated through it store a reference to it.

`free` returning a result allows allocators to diagnose foreign allocations in checked configurations. Specialized infallible release operations may exist when ownership is statically obvious.

## Explicit durable allocation

Any operation that creates, grows, clones, resizes, or destroys durable owned memory receives an allocator explicitly:

```forge
fn decode_image(bytes: u8[], allocator: &mut Allocator)
    -> Result[Image, DecodeError];
```

A larger owning object does not make durable allocation ambient. If spawning an entity allocates, its API exposes the allocator:

```forge
fn spawn(
    world: &mut World,
    allocator: &mut Allocator,
    spec: EntitySpec
) -> Result[Entity, AllocError];
```

Likewise, being a method or operating on a receiver does not authorize allocation through an allocator hidden inside that receiver. Ordinary data structures must not retain allocator capabilities for this purpose.

The exception is an object whose defined purpose is to implement a memory domain itself, such as `Allocator`, `Arena`, `ObjectCache`, a slab allocator, or an explicitly named pool/memory-domain manager. Such objects may retain lower-level provider capabilities as implementation state. Clients still pass the resulting allocator explicitly to ordinary durable-allocation APIs.

## Allocation provenance

Backing memory retains allocation-domain provenance even though the owning value does not store the allocator capability.

A later `resize` or `free` must receive:

- the same allocator domain that created the block; or
- another allocator/provider explicitly documented as compatible with that domain.

Using an incompatible allocator is a contract violation. Checked/debug providers should report `ForeignAllocation`, `CorruptState`, or an equivalent defined diagnostic rather than silently corrupting memory.

Moving a collection/value transfers the caller's responsibility to preserve this allocator-domain provenance. The collection itself does not spend storage retaining allocator identity.

## Scratch allocation

`context.scratch` is a per-execution-context allocator for memory that cannot escape its scope/call tree.

```forge
fn parse_number(text: str) -> Result[f64, ParseError] {
    val mark = context.scratch.mark();
    defer context.scratch.release(mark);

    val temp = tokenize_temp(text, context.scratch.allocator())?;
    return interpret_number(temp);
}
```

Returning a pointer/reference/slice into scratch memory beyond the valid scratch region is invalid and should be diagnosed when the compiler can prove it.

Scratch allocation cannot be used to satisfy an API returning durable owned data unless that data is copied into storage obtained from an explicitly supplied durable allocator before return.

## Context

Context is deliberately narrow:

```forge
struct Context {
    scratch: &ScratchArena;
    logger: &Logger;
    clock: &Clock;
    random: &Random;
    trace: &TraceSink;
}
```

It is not an application service locator. Application objects such as databases, worlds, renderers and users are explicit parameters.

Scoped override:

```forge
with context {
    :scratch &test_scratch
    :logger &test_logger
} {
    run_test();
}
```

No durable-allocation API may silently obtain an allocator from context.

## Arenas as memory domains

"Arena" denotes a lifetime/ownership domain; policy determines allocation behavior.

Standard policies should include:

- linear/bump variable-size arena;
- stack/LIFO arena;
- fixed-size object pool;
- slab/multiple size classes;
- free-list/coalescing arena;
- fixed-buffer/static arena;
- thread-local arena.

Bulk `reset`/`release(mark)` is separate from whether individual blocks can be freed.

An arena is a memory-domain object, not an ordinary value container. APIs that allocate ordinary durable values from an arena-derived allocator still receive that allocator explicitly.

## Fixed-size pools

Generated concrete pools are encouraged in v1:

```forge
#forge/type {
    :template std/pool
    :element Connection
    :name ConnectionPool
}
```

A pool is explicitly a memory-domain manager. It may retain the provider/backing domain required to implement its fixed-size allocation policy. This is deliberately different from `List`, `String`, `HashMap`, and other ordinary containers.

## Vectors and ordinary collections

The fundamental vector is allocator-independent as a value:

```forge
struct ListU32 {
    block: MemoryBlock?;
    len: usize;
    capacity: usize;
}
```

Growth and destruction operations receive the allocator that owns the backing buffer:

```forge
list_u32_push(&mut list, &mut allocator, value)?;
list_u32_try_reserve(&mut list, &mut allocator, additional)?;
list_u32_destroy(&mut list, &mut allocator);
```

Non-allocating operations do not take an allocator:

```forge
list_u32_len(&list);
list_u32_get(&list, index);
list_u32_pop(&mut list);
```

Using an incompatible allocator for a later resize/free is a programmer error and should produce `AllocError::ForeignAllocation`, `AllocError::CorruptState`, or a defined contract trap when detectable.

There is intentionally **no `ManagedVec`/managed collection wrapper in the v1 standard model whose purpose is to retain an allocator and hide it from subsequent calls**. Code that wants convenience should keep allocator and collection as separate fields in a higher-level application object and pass the allocator explicitly:

```forge
struct DecoderState {
    bytes: ListU8;
    // other decoder state
}

fn append(
    state: &mut DecoderState,
    allocator: &mut Allocator,
    value: u8
) -> Result[void, AllocError] {
    return list_u8_push(&mut state.bytes, allocator, value);
}
```

This rule applies to dynamic strings, lists, hash maps, hash sets, deques, trees, priority queues, buffers, and other ordinary owning containers.

## Kernel allocation

All primitive allocation remains fallible. Kernel code distinguishes allocators with different execution constraints explicitly:

```forge
allocate_task(&mut task, &mut normal_allocator)?;
allocate_irq_record(&mut record, &mut interrupt_allocator)?;
allocate_dma_buffer(&mut buffer, &mut dma_allocator)?;
```

Interrupt/critical paths should preferentially preallocate pools so allocation has bounded behavior.

`Result` values from allocation/growth operations are `@must_use` by default.

The kernel does not receive an exception to the explicit-allocator rule. This is particularly useful in kernel code because the call site makes blocking, emergency, DMA, NUMA, or other memory-domain choices reviewable.
