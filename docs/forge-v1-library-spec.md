# Forge v1 Library and Execution Environment Specification

**Status:** v1 design baseline.  
**Relationship to the language:** this document specifies the standard library layers, compilation-unit model, runtime ABI boundary, allocation discipline, and hosted/freestanding execution environments. It does **not** add new Forge language syntax.

## 1. Standard library layers

Forge v1 defines two standard-library layers:

```text
core     freestanding standard library
std      hosted standard library layered on core
```

There is no library named `nostd`. `--no-std` selects a freestanding build environment in which the hosted `std` library is not supplied.

## 2. `core`

`core` is available in hosted and freestanding builds. It may depend only on Forge language semantics, target ABI facts, compiler/runtime primitives explicitly defined by the Forge implementation, and resources passed explicitly by the caller.

`core` must not require an operating system, libc, filesystem, socket API, process model, default heap, conventional `main()` function, environment variables, or language-level garbage collector.

The normative design and planned contents of `core` are described in `core-library.md`.

## 2.1 Freestanding companion libraries

Forge may ship optional freestanding libraries layered on `core`. They are ordinary compilation units, remain available in `--no-std` builds, and are imported explicitly rather than injected as a prelude.

The initial OS-foundation set uses these logical module names:

```text
core.mem
core.bits
core.mmio
core.atomic
core.sync
core.fixed
core.intrusive
core.layout
core.io
core.target
core.hash
```

These libraries may depend on `core`; `core.sync` may additionally depend on `core.atomic`. They must not depend on hosted `std`, libc, Cosmic, a scheduler, a default heap, or platform services that are not passed explicitly or represented by a compiler/runtime primitive.

The `core.*` namespace does **not** mean their declarations are implicitly part of the `core` root module. For example:

```forge
import core;
import core.mem;
import core.atomic;
```

imports three explicit semantic compilation units. A program pays source/API dependency only for the units it actually imports.

The shipped bootstrap files live under `lib/freestanding/`, but source compatibility is defined by logical module names rather than that filesystem path. Their roadmap and acceptance criteria are documented in `freestanding-libraries.md`.

## 3. `std`

`std` imports and builds on `core`. It may rely on a target's hosted platform contract and can provide startup, terminal/stream I/O, filesystems, networking, clocks, entropy, OS threads, process services, and hosted allocator providers.

`std` may provide convenient constructors for allocators, arenas, pools, or application memory domains. It does **not** establish a hidden process-global allocator for ordinary library APIs.

An API belongs in `core`, not `std`, when its semantics can be implemented without assuming hosted services and all required resources can be supplied explicitly.

## 4. Runtime ABI

Compiler lowering may require a small runtime ABI. That ABI is below both `core` and `std` and must not imply hosted execution.

All defined panic/trap paths converge on one canonical non-returning runtime provider described in `runtime-abi.md`. The portable conceptual interface is:

```text
__forge_panic(info: &core.PanicInfo) -> never
```

`core` may raise a panic or defined trap but does not own the final panic policy. A hosted build normally receives the provider from `std`; a freestanding final artifact provides its own implementation.

The runtime ABI also includes low-level memory transfer or target primitives where compiler lowering requires them. Such primitives must not imply an OS, heap, scheduler, or C runtime.

## 5. Build environments

Hosted is the default execution environment:

```text
forge build program.fg
```

Freestanding execution is selected with:

```text
forge build --no-std program.fg
```

A `--no-std` program continues to use the Forge language and `core`. It need not define `main()` and may provide its own entry point and runtime hooks.

## 6. Bootstrap compilation-unit model

For the bootstrap compiler:

```text
one source file = one module = one library compilation unit
```

Logical library names are bound to root source files by the build invocation or compiler distribution:

```text
forge build main.fg \
    --library core=lib/core.fg \
    --library std=lib/std.fg
```

Source imports semantic names:

```forge
import core;
import std;
```

Source code does not import library implementation files by relative filesystem path.

## 7. Compilation graph

The compiler shall construct the semantic dependency graph before linking the final artifact:

```text
main
  -> std
       -> core
```

For each compilation unit the implementation must:

1. parse the unit independently;
2. collect its public declarations;
3. resolve imported module interfaces;
4. perform semantic/type checking;
5. make only public declarations available to importers;
6. reject unresolved imports and unsupported dependency cycles;
7. link/code-generate the requested final artifact.

The compiler may cache interface or compiled-unit results. Such caching must not alter source semantics.

## 8. Standard-library lookup

A compiler distribution may implicitly bind the logical names `core` and `std` to its shipped library roots. This is a build-system convenience, not a distinct import language feature.

Explicit library mappings must be possible so a compiler, kernel, test suite, or alternate platform runtime can select another implementation deliberately.

## 9. Prelude

Forge v1 bootstrap defines no broad implicit standard-library prelude. Primitive language types and compiler-defined type constructors remain directly available. Standard-library declarations require imports.

## 10. Allocation

Forge v1 has no language-level global allocator and no implicit `malloc` operation.

### 10.1 Explicit allocator rule

For standard-library and ordinary application data structures, **every operation that allocates, reallocates, grows, clones owning storage, or frees durable memory must receive the allocator capability explicitly as a parameter at the call site**.

This is the default and normative Forge v1 allocation discipline.

Examples:

```forge
list_u8_push(&mut bytes, &mut allocator, value)?;
hash_map_string_u64_put(&mut map, &mut allocator, key, value)?;
list_u8_destroy(&mut bytes, &mut allocator);
```

Operations that provably cannot allocate or free do not take an allocator:

```forge
list_u8_len(&bytes);
list_u8_get(&bytes, index);
hash_map_string_u64_contains(&map, key);
```

### 10.2 Collections and ordinary owning values do not retain allocators

Generated collections and ordinary owning standard-library values **must not contain `Allocator`, `&Allocator`, or `&mut Allocator` fields merely so future operations can allocate**.

For example, this is the required shape:

```forge
pub struct ListU8 {
    block: MemoryBlock?;
    len: usize;
    capacity: usize;
}
```

and not:

```forge
pub struct ListU8 {
    allocator: &mut Allocator; // forbidden for ordinary collection ownership
    ...
}
```

The same rule applies to `List*`, dynamic strings, `HashSet*`, `HashMap*`, deques, trees, priority queues, and similar standard-library containers.

A specialized memory-management object such as an `Allocator`, `Arena`, slab allocator, object cache, or explicitly named memory-domain manager may itself retain backing-provider capabilities as part of implementing that memory domain. This exception does **not** turn normal data structures into allocator-owning wrappers.

### 10.3 No default/global allocator fallback

An allocating standard-library operation must not silently fall back to:

- a process-global allocator;
- a thread-local allocator;
- `context.scratch`;
- libc `malloc`;
- a hosted runtime heap;
- an allocator stored earlier in the collection.

If an API allocates durable memory, the allocator must be visible in that operation's signature.

Temporary scratch allocation remains a separate, non-escaping facility and cannot be used to satisfy durable ownership.

### 10.4 General allocator architecture

The common allocator interfaces and object-cache algorithms belong in `core` so they can be shared between kernels and hosted programs.

Forge supports both general variable-sized allocation and fixed-size object caches. Variable-sized allocation is the explicit Forge equivalent of the traditional malloc workload; fixed-size caches optimize known object shapes and may also be used internally as size classes by a general allocator.

The environment supplies the backing resource:

```text
core allocation algorithms
       |
       +-- kernel VM/page provider
       +-- hosted virtual-memory provider
       +-- firmware/static-region provider
       +-- test provider
```

Fixed-size object allocation is a first-class standard-library facility. `core` shall provide an object-cache abstraction capable of obtaining slabs/extents from an explicit backing arena and returning fixed-size objects efficiently.

The common algorithm must not depend on kernel-only VM APIs or hosted-only system calls. The detailed architecture is specified in `core-allocation.md`.

## 11. Allocation failure

Primitive allocation is fallible. Standard allocation interfaces return `Result` (or an equivalent explicitly fallible Forge value). A higher layer may deliberately convert failure into panic, but `core` must not make this an implicit global policy.

Allocation failure therefore remains distinct from the panic ABI. `core.PanicKind::AllocationFailure` is available only for callers or wrappers that intentionally choose fatal allocation semantics.

## 12. Kernel and user-mode reuse

Forge standard-library design should maximize source sharing across execution domains. In particular, allocation, object caches, collections, parsing, hashing, and other environment-independent algorithms should be implementable once in `core` and compiled for both Cosmic kernel and user-mode targets.

Kernel/user divergence belongs at explicit provider boundaries such as:

- backing arena acquisition;
- synchronization implementation;
- CPU/thread-local caching policy;
- memory-pressure notification;
- panic/diagnostic sink;
- platform I/O.

The explicit allocator call-site rule is identical in kernel and user mode.

## 13. Freestanding panic contract

A final `--no-std` executable, kernel, firmware image, or boot environment does not receive a hosted panic implementation.

Libraries may contain operations that can panic or trap. They do not define the process/kernel policy. The final freestanding environment supplies the canonical panic provider, which receives allocation-free `core.PanicInfo` and never returns.

Required defined-trap classes include explicit panic, assertion failure, bounds failure, checked integer overflow, divide-by-zero, invalid checked shifts, and compiler-generated unreachable/invariant failures.

Forge v1 panic does **not** unwind the stack. `defer` is not executed as panic unwinding. Panic terminates the current execution unit according to the environment's provider policy.

An implementation may offer a build option that synthesizes a minimal trap/halt provider for extremely small firmware, but this is a build-system policy and does not change `core` semantics.

## 14. Entry points

Hosted `std` may provide conventional startup and a `main()` contract. `--no-std` does not require or synthesize a conventional `main()` entry point.

Kernel, firmware, bootloader, and embedded targets may bind whatever platform entry symbol is required. Entry-point selection and panic-provider selection are independent.

## 15. ECS relationship

The fixed-size allocation facilities in `core` are data-oriented and intentionally useful for ECS implementations, but are not themselves an ECS.

`ObjectCache` provides homogeneous fixed-size storage, predictable layout, and efficient object reuse. Entity identity, component membership, archetype/query semantics, and system scheduling belong to an ECS layer built above `core`.

## 16. Evolution

The bootstrap uses single-file roots `lib/core.fg` and `lib/std.fg`. A later Forge version/toolchain may allow a logical library to contain multiple modules and files. That evolution must preserve semantic import names and must not make filesystem layout part of the source-language compatibility contract.
