# Forge Runtime ABI

**Status:** v1 design baseline.

This document defines the minimal runtime contract required beneath `core` and `std`. It is not a hosted runtime and must remain usable by kernels, firmware, boot environments, and ordinary hosted programs.

## 1. Principle

Forge libraries may raise defined traps and panics without knowing how the final program reports or terminates them.

All defined panic/trap paths converge on one canonical non-returning runtime hook:

```text
__forge_panic(info: &core.PanicInfo) -> never
```

The exact symbol mangling is implementation-defined until the Forge ABI is frozen, but a final linked artifact must have exactly one canonical panic implementation.

`core` may construct and pass `PanicInfo`, but must not provide the environment policy for reporting, rebooting, halting, debugging, or terminating a process.

### Panic is non-unwinding in Forge v1

The v1 runtime ABI does not require stack unwinding, exception tables, a personality routine, or heap allocation for panic. `__forge_panic` does not return and ordinary `defer` semantics must already have been lowered where the language guarantees them; panic itself is terminal for the current execution unit.

A future ABI may define an optional recovery/unwind facility, but it must not become a requirement for freestanding `core` or silently change v1 panic behavior.

This is intentionally suitable for kernels and embedded systems: a panic path can remain valid before allocators, schedulers, filesystems, consoles, or userspace exist.

## 2. Freestanding behavior

A `--no-std` executable or kernel image does not receive a hosted panic implementation.

The final environment must provide the panic hook when generated code can reach any defined panic/trap path. Examples include:

- explicit `panic(...)`;
- assertion failure;
- checked integer overflow;
- divide-by-zero;
- invalid checked shift count;
- bounds-check failure;
- compiler-generated unreachable/invariant traps.

A freestanding provider may:

- halt forever;
- enter a debugger;
- print through a serial console and halt;
- invoke a kernel panic path;
- reboot firmware;
- record crash state and reset.

It must never return normally.

A final freestanding image that can reach a panic path but supplies no handler is a build/link error. Supplying more than one canonical handler is also an error.

## 3. Hosted behavior

`std` supplies the normal hosted implementation. A typical implementation writes a diagnostic to the process error stream when available and terminates the process.

Hosted implementations may provide richer diagnostics such as symbols or backtraces, but those are outside the `core` ABI. `PanicInfo` itself must stay allocation-free and usable before a heap, scheduler, filesystem, or terminal exists.

## 4. Panic information

The initial portable record is:

```forge
pub enum PanicKind {
    Explicit,
    Assertion,
    Bounds,
    IntegerOverflow,
    DivideByZero,
    InvalidShift,
    Unreachable,
    AllocationFailure,
}

pub struct PanicLocation {
    file: str;
    line: u32;
    column: u32;
}

pub struct PanicInfo {
    kind: PanicKind;
    message: str;
    location: PanicLocation?;
}
```

The record contains borrowed/static data only. Constructing or dispatching a panic must not require allocation.

`AllocationFailure` exists for code that deliberately converts a failed allocation into panic. Primitive `core` allocation APIs do **not** do this automatically.

## 5. Allocation failure

Allocation failure and panic are intentionally separate mechanisms.

Primitive allocation returns:

```text
Result[T, core.AllocError]
```

Callers may propagate, retry, reclaim, fall back, or deliberately panic. This is important for kernels and embedded systems where an allocation failure may be recoverable in one subsystem but fatal in another.

## 6. Entry points

`--no-std` does not imply a `main()` ABI. The environment controls the entry point.

Examples include a kernel bootstrap symbol, firmware reset vector, bootloader entry, or an ordinary hosted executable entry supplied by platform startup code.

Entry-point selection and panic-provider selection are separate concerns.

## 7. Comparison model

Forge deliberately takes two useful ideas from existing systems languages:

- Rust `no_std`: `core` can invoke panic but the final freestanding binary supplies the panic handler.
- Zig: the final root/program can override the canonical panic policy rather than libraries embedding one.

Forge keeps the v1 ABI explicitly non-unwinding and does not require a separate freestanding standard library. `core` remains the same library in kernel, embedded, and hosted builds; only the runtime provider changes.

## 8. Testing requirements

The standard library/runtime test suite must verify at least:

1. `core` contains no dependency on `std`;
2. `PanicInfo` is available from `core` alone;
3. panic dispatch requires no allocator;
4. checked-trap classes map to stable `PanicKind` values;
5. allocation failure remains a returned error unless explicitly converted to panic;
6. a freestanding final artifact can supply a custom panic provider;
7. a missing required provider is diagnosed during final linking/building rather than becoming an unresolved runtime accident;
8. duplicate panic providers are rejected;
9. the panic hook is non-returning;
10. panic does not require unwind tables or an unwinder in v1;
11. hosted and freestanding implementations can use the same `core` code.