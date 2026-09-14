# Forge library and execution-environment model

**Status:** Forge v1 library/compilation model baseline.

Forge distinguishes the language, compiler runtime ABI, freestanding `core` library, and hosted `std` library. `nostd` is not a separate library.

## Layers

```text
Forge language
    |
    +-- compiler intrinsics / lowering
    |
    +-- runtime ABI
    |      panic, bounds failure, overflow failure, low-level copies
    |
    +-- core
    |      freestanding library; no OS or heap assumption
    |
    +-- std
           hosted services layered on core
```

### Language

Primitive types and compiler-defined constructors such as integers, `bool`, arrays, slices, `T?`, and `Result[T,E]` are language facilities. They do not disappear in freestanding builds.

### Runtime ABI

The runtime ABI is the minimal contract required by compiler lowering. It is not an operating-system runtime and does not imply libc, processes, files, sockets, threads, or a heap.

Candidate hooks include:

```text
__forge_panic
__forge_bounds_fail
__forge_overflow_fail
__forge_memcpy
__forge_memmove
__forge_memset
```

A kernel, boot environment, firmware image, or hosted runtime may supply these hooks differently.

### `core`

`core` is Forge's freestanding standard library. It may assume only Forge language semantics, target ABI facts, and explicitly supplied resources.

`core` must not require:

- an operating system;
- a process model;
- libc;
- a filesystem;
- sockets;
- threads;
- environment variables;
- a default/global heap allocator;
- a conventional `main()` entry point.

`core` is available to both hosted and `--no-std` programs.

### `std`

`std` is the hosted standard library and imports `core`. It may rely on the platform contract selected by the target/runtime implementation.

Hosted facilities may include:

- program startup and normal `main()` entry;
- stdout/stderr;
- process exit;
- filesystem and paths;
- networking;
- OS threads and synchronization;
- clocks and entropy;
- a process allocator implementation.

`std` must not redefine facilities already provided by `core`; it layers hosted policy and services on top.

## Build modes

Normal hosted build:

```text
forge build program.fg
```

Freestanding build:

```text
forge build --no-std program.fg
```

`--no-std` means that the hosted library and hosted startup/runtime policy are not injected. `core` remains available.

A future compiler-development option may permit omitting `core` as well, but this is not the ordinary freestanding model.

## Initial compilation-unit model

The bootstrap rule is deliberately simple:

```text
one source file = one module = one library compilation unit
```

The compiler invocation maps logical library/module names to root source files:

```text
forge build main.fg \
    --library core=lib/core.fg \
    --library std=lib/std.fg
```

Source imports logical names, never relative implementation paths:

```forge
import core;
import std;
```

The compiler constructs a directed module graph, rejects cycles that are not explicitly supported, resolves exported declarations, checks modules independently, and then links the resulting program or library artifact.

The compiler distribution may provide implicit defaults equivalent to:

```text
core -> $FORGE/lib/core.fg
std  -> $FORGE/lib/std.fg
```

but `core` and `std` still use the ordinary module/library mechanism rather than a separate parser or language feature.

## Visibility and API boundary

Module-private is the default. `pub` declarations form the module's public interface. Importing a module does not paste source text and does not expose private declarations.

The long-term model may permit a logical library to contain multiple modules/source files. That does not change the rule that imports refer to semantic module names rather than filesystem paths.

## No implicit prelude initially

Forge v1 bootstrap does not inject a broad standard-library prelude. Language primitives remain directly visible; library facilities require explicit imports. A small prelude may be standardized later only after repeated usage demonstrates that it is worthwhile.

## Hosted and freestanding summary

| Facility | Hosted | `--no-std` |
| --- | --- | --- |
| Forge language | yes | yes |
| compiler intrinsics | yes | yes |
| runtime ABI | hosted implementation | target/program implementation |
| `core` | yes | yes |
| `std` | yes | no |
| OS required | normally yes | no |
| allocator required | only by APIs that allocate | only by APIs that allocate |
| `main()` required | normal hosted convention | no |

## Design principle

The same portable algorithms should be usable in a Cosmic kernel, firmware image, bootloader, hosted program, and test harness. Environment-specific policy belongs behind explicit backing providers and platform modules, not in the allocator algorithms or fundamental containers.