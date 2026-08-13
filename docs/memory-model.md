# Forge v1 Memory and Allocation Model

This document specifies standard conventions and library architecture around the language's explicit memory model.

## Principles

1. No mandatory garbage collector.
2. No hidden allocation for ordinary value operations.
3. Durable/escaping allocation identifies an allocator or owning memory domain.
4. Temporary non-escaping allocation may use `context.scratch`.
5. Primitive allocation is fallible.
6. Wrong-allocator operations should be detected where reasonably possible rather than becoming silent corruption.
7. Large systems should organize memory by lifetime and data domain, not by individual object ownership alone.

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

`free` returning a result allows allocators to diagnose foreign allocations in checked configurations. Specialized infallible release operations may exist when ownership is statically obvious.

## Explicit durable allocation

A function returning owned memory takes an allocator unless the receiver already owns one:

```forge
fn decode_image(bytes: u8[], allocator: &Allocator)
    -> Result[Image, DecodeError];
```

An ECS/world operation can instead use the world's memory domain:

```forge
fn spawn(world: &mut World, spec: EntitySpec)
    -> Result[Entity, AllocError];
```

The allocation dependency is explicit through `world`.

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

## Fixed-size pools

Generated concrete pools are encouraged in v1:

```forge
#forge/type {
    :template std/pool
    :element Connection
    :name ConnectionPool
}
```

The generated pool groups equal-sized objects and exposes fallible `acquire` plus `release`.

## Vectors

The fundamental vector should be unmanaged:

```forge
struct Vec_u32 {
    data: *u32?;
    len: usize;
    cap: usize;
}
```

Growth operations require the allocator that owns the backing buffer:

```forge
vec.push(&arena, value)?;
vec.reserve(&arena, additional)?;
vec.free(&arena)?;
```

Using a different allocator for a later resize/free is a programmer error and should produce `AllocError::ForeignAllocation` or a defined contract trap when detectable.

Convenience wrapper:

```forge
struct ManagedVec_u32 {
    vec: Vec_u32;
    allocator: &Allocator;
}
```

No function pointers are needed in the managed vector; its behavior is statically known. It delegates storage operations to the contained allocator.

## Kernel allocation

All primitive allocation remains fallible. Kernel code must be able to distinguish allocators with different execution constraints rather than hiding them in flags at every call:

```forge
normal_memory.alloc(...)?;
interrupt_memory.alloc(...)?;
dma_memory.alloc(...)?;
```

Interrupt/critical paths should preferentially preallocate pools so allocation has bounded behavior.

`Result` values from allocation/growth operations are `@must_use` by default.
