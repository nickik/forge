# Forge compiler evolution TODO

This is the top-level implementation order for the Forge compiler after C14.
The versioned `C12_TODO.md` through `C15_TODO.md` files remain the detailed
acceptance records for their individual milestones.

## Rule for advancing work

For each slice, use one semantic fixture, one FIR/lowering assertion where that
adds value, and one native proof on the target that claims support. Keep target
capability boundaries explicit: a language feature is not automatically
available on every backend merely because the frontend accepts it.

## 1. Preserve the C14 compiler contract

- [ ] Keep the full workspace, conformance, Clippy, AArch64 hosted-native and
      RISC-V/QEMU gates green on every compiler checkpoint.
- [ ] Maintain the C14 completeness matrix as the authoritative claim ledger;
      downgrade a row when a regression invalidates its executable evidence.
- [ ] Turn each remaining backend `Unsupported*` path into either a documented
      target boundary or a named milestone with a focused regression.
- [ ] Continue rejecting compiler-internal sentinel types at the FIR boundary.

## 2. Close the remaining native C14 matrices

- [ ] `duration`: native argument, return, local, load and store tests.
- [ ] slices: mutable use, aggregate-contained slices and return-value ABI
      execution.
- [ ] bitstructs: production native read/write/check lowering, including
      overflow/range diagnostics.
- [ ] globals: mutable storage, address-taking, aggregates and pointer
      relocation execution.
- [ ] `defer`: early `return`, loop exits, nested ordering and `?` propagation.
- [ ] `match`: map-protocol execution and more mixed aggregate payload cases.
- [ ] distinct types: conversion and ABI execution coverage.

## 3. Harden ABI and object correctness

- [ ] Add cross-target ABI fixtures for mixed scalar/aggregate calls, hidden
      returns, recursion and indirect calls.
- [ ] Define floating-point aggregate ABI classes for native targets before
      enabling float aggregate calls beyond the current scalar/field coverage.
- [ ] Audit object sections, relocations, visibility and deterministic symbol
      naming for every supported target.
- [ ] Add malformed-object, unresolved-symbol and relocation-overflow negative
      tests at each object/image boundary.

## 4. Target capability roadmap

### AArch64 hosted native

- [ ] Complete `f32`/`f64` call/return ABI, aggregate ABI and exceptional-value
      (`NaN`, signed zero, conversion) execution matrices.
- [ ] Keep hosted executable fixtures as the acceptance proof, not object
      emission alone.

### RISC-V 64

- [ ] Maintain object and QEMU execution parity for completed scalar,
      aggregate, global and runtime-initialization features.
- [ ] Add float only when the RISC-V ISA/ABI configuration and execution
      contract are intentionally selected and tested.

### SIA32 — C15 and later

- [ ] Complete Cranelift M5 integer production lowering and Forge integration
      before broadening Forge semantics on SIA32.
- [ ] Retain explicit rejection for `f32`/`f64` until the C15 prerequisites in
      `C15_TODO.md` are complete.
- [ ] Add native SIA image and Lighting execution tests for every feature that
      becomes supported; object creation alone is insufficient.
- [ ] Keep SIA-specific ABI and lowering changes isolated from AArch64/RISC-V
      unless a shared defect is proven.

## 5. Compiler architecture and diagnostics

- [ ] Make diagnostics consistently carry the failing source span, stable code,
      expected/actual semantic facts, and a regression asserting the code.
- [ ] Improve FIR verifier diagnostics so invalid producer contracts identify
      the function, block, instruction and value involved.
- [ ] Keep lowering phases one-way: parser/HIR/typechecking decisions must be
      represented in FIR rather than reconstructed in codegen.
- [ ] Add compiler debug dumps for typed HIR, FIR, ABI decomposition, CLIF and
      object plans behind stable, testable flags.

## 6. Build, package and Cosmic readiness

- [ ] Finish freestanding `:kernel` validation: symbols, sections, relocations,
      entry contract and rejection of hosted dependencies.
- [ ] Keep package/module visibility and dependency ordering exercised through
      real multi-package builds.
- [ ] Define the Forge-to-Cosmic kernel/userland compile contract once the
      target-independent freestanding object path is stable.
- [ ] Validate each claimed Forge backend against one representative Cosmic
      component only after its ordinary compiler gate is green.

## Explicitly deferred

- SIA32 floating point before C15 is complete.
- `select` / channels, escaping closures and tail calls until they receive a
  separate language and runtime milestone.
- A second SIA machine-code backend: Forge continues to use Cranelift SIA32.
