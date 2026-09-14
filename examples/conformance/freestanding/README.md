# Freestanding conformance suite

This directory reserves normative tests for Forge `--no-std`, the runtime panic ABI, and `core` allocation facilities.

The manifest is intentionally ahead of the current linker/runtime implementation. A test may name capabilities in `:requires`; it becomes executable as soon as those capabilities exist. Unsupported infrastructure must not be counted as a semantic pass.

## Test providers

The eventual harness should supply deterministic providers:

- `:custom-panic` — minimal non-returning freestanding panic implementation;
- `:test-panic-recorder` — records the received `PanicInfo` to harness-visible state and terminates the execution instance;
- `:failing-arena` — rejects every allocation with `OutOfMemory`;
- `:test-arena` — bounded byte region with deterministic addresses/offsets;
- `:kernel-like-arena` — page-granular provider with `MayWait`/`NoWait` behavior;
- `:hosted-like-arena` — extent provider modeling a hosted VM source.

## Panic invariants

Tests must establish that:

1. `core` can cause panic/trap dispatch without importing `std`;
2. `PanicInfo` construction requires no allocation;
3. a freestanding final artifact owns the panic provider;
4. the provider never returns;
5. checked runtime traps report stable `PanicKind` values;
6. panic does not unwind or run `defer` as stack-unwinding cleanup;
7. ordinary allocation failure is returned as `AllocError`, not routed to panic unless a caller explicitly requests fatal behavior.

## Allocation invariants

Both allocation styles are required:

- arbitrary-size `Allocator` behavior comparable to a traditional malloc/realloc/free facility, but explicit and fallible;
- fixed-size `ObjectCache` behavior backed by explicit arenas.

The same upper allocator/cache implementation should be exercised over multiple fake backing providers. This is the regression test for the kernel/user-space sharing goal.

## Promotion rule

Once the linker/runtime/allocator can execute one of these cases, move it into the normal automated conformance path rather than replacing or weakening the expected behavior.
