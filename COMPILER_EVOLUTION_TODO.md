# Forge compiler evolution TODO

## 2026-09 M27 checkpoint

- [x] Forge emits and links the freestanding SIA32 image used by Cosmic's native M27 boot.
- [x] SIA32 privileged operations used by Cosmic lower through production Cranelift: SREAD/SWRITE, TLBFENCE, SYNC.I and TRAP.
- [x] SIA32 direct-call literals preserve the required 4-byte function alignment through final image layout.
- [x] The merged Forge image is consumed by LightingSimulation's end-to-end Cosmic boot proof.
- [x] M27 proof reaches Cosmic, installs VMCTX/page tables, enables translation, enters and returns from TRAP 0x27, and halts intentionally.
- [~] Next vertical target: compile the smallest System Task/userspace image and support the kernel/user crossing required to run it.\n  - [x] M28.1 add a dedicated freestanding SIA32 user-image compiler surface and deterministic image contract.\n  - [ ] Consume Cosmic's `user/native/m28_probe.fg` and validate TRAP 0x40 plus post-SRET proof-store code in LightingSimulation.


This is the top-level implementation order for the Forge compiler after C14.
The versioned `C12_TODO.md` through `C15_TODO.md` files remain the detailed
acceptance records for their individual milestones.

## End goal

The compiler program is complete only when Forge can build the relevant Cosmic
kernel and userspace components for SIA32, produce a loadable SIA image, and
run that image with observable correct behavior on the Lighting simulator.
Hosted AArch64 and RISC-V lanes are supporting compiler evidence; they do not
replace native SIA/Lighting execution.

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

- [x] `duration`: native argument, return, local, load and store tests.
- [x] slices: explicit array-reference views, mutable use, aggregate-contained
      slices and return-value ABI execution.
- [x] bitstructs: production native read/write/check lowering, including
      overflow/range diagnostics and `u8`/`u16`/`u32`/`u64` value ABI.
- [x] globals: mutable storage, address-taking, whole-value aggregate mutation
      and static function/global pointer relocation execution.
- [x] globals follow-up: direct field places rooted in globals, including
      reads, stores and mutable field addresses.
- [x] globals follow-up: static aggregate-pointer initializer execution on
      AArch64 and RISC-V.
- [x] `defer`: direct and `?` early return, loop exits and nested LIFO
      ordering execute natively.
- [x] `match`: resolved map-protocol lookup/closed-key calls execute natively,
      including required and optional bindings.
- [x] `match`: mixed fieldless, narrow-scalar and multi-field tagged payloads
      execute across argument and return ABI boundaries.
- [x] distinct types and aliases: explicit conversion and value ABI execution.

## 3. Harden ABI and object correctness

- [x] Add cross-target ABI fixtures for mixed scalar/aggregate calls, hidden
      returns, recursion and indirect calls.
- [ ] Define floating-point aggregate ABI classes for native targets before
      enabling float aggregate calls beyond the current scalar/field coverage.
- [ ] Audit object sections, relocations, visibility and deterministic symbol
      naming for every supported target.
- [ ] Add malformed-object, unresolved-symbol and relocation-overflow negative
      tests at each object/image boundary.
  - [x] SIAO32 parsing/linking pins exact diagnostics for malformed objects,
        unresolved symbols and overflowing `ABS32` relocations.
  - [x] Static function/global pointer relocations reject missing targets before
        object emission on AArch64 and RISC-V.
  - [x] Native ELF emission pins unresolved function/global/internal-label
        diagnostics and rejects text/static-data relocation ranges that escape
        their owning section before writing an object.

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

- [x] Complete the integer/privileged production lowering required by the merged Cosmic M27 native boot path.
- [ ] Expand SIA32 support only when demanded by the next Cosmic vertical slice; the immediate requirement is a freestanding System Task/user image and syscall ABI, not floating point.
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
  - [x] AArch64 Forge kernel objects export the manifest-selected entry under
        its exact platform symbol and verify ELF sections, call relocations,
        and the absence of undefined hosted/runtime imports.
  - [x] Repeat the contract with the representative Cosmic M27 early-kernel SIA32 image and execute it through LightingSimulation.
  - [ ] Extend the contract to a separate freestanding System Task/user image and kernel/user ABI crossing.
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
