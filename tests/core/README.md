# Forge core-library tests

This directory is the normative test plan for Forge `core`, including freestanding panic handling and allocation.

The suite is intentionally split into three levels:

1. **contract tests** — source/library/runtime invariants that can be checked before full execution support exists;
2. **check/link tests** — compiler obligations such as exactly one freestanding panic handler;
3. **run tests** — executable semantics such as trap classification, allocation failure, and object-cache geometry.

`contracts.fdn` is the canonical manifest. Implementations may report cases as unsupported during bootstrap, but unsupported is never conforming.

## Panic requirements

A freestanding final artifact must provide exactly one non-returning panic handler whenever panic paths are reachable. `core` itself provides `PanicInfo`, not termination policy. Forge v1 panic does not require unwinding.

Hosted `std` supplies the normal hosted handler. `--no-std` does not.

## Allocation requirements

Both allocation styles are required:

- `Allocator`: arbitrary-size, malloc-like allocation with explicit size/alignment and fallible `Result` semantics;
- `ObjectCache`: fixed-size object allocation backed by an `Arena`.

OOM is an ordinary `AllocError` until a caller explicitly promotes it to panic.

## Current executable reference checks

CForge's `cforge.library-contract-test` currently exercises the bootstrap-checkable subset directly: dependency direction, panic ABI shape, allocation API presence, power-of-two/alignment rules, and object-cache stride/capacity examples.

As Forge/CForge gain module linking, runtime hooks, structs, pointers, and allocators, the corresponding entries in `contracts.fdn` should become full check/link/run tests rather than static contract checks.
