# FIR -> CLIF Backend Plan

## Compiler boundary

**CLIF is a code-generation IR, not a Forge semantic compiler layer.**

All Forge language semantics must be resolved by Typed HIR and FIR. FIR is the last Forge-owned semantic IR. FIR -> CLIF performs only mechanical representation lowering required by Cranelift.

The backend-facing package boundary is `forge-fir`. Code generators depend on this facade, not on `forge-frontend`. The facade exports FIR plus only the foundational IDs/types that occur in FIR fields; AST, HIR, Typed HIR, parser, resolver and type-checker implementation structures are not backend APIs.

The codegen layer must not perform or reconstruct:

- name or overload resolution;
- default/named argument normalization;
- pattern usefulness or match planning;
- closure capture discovery;
- unsafe authorization;
- context-slot resolution;
- collection-pattern protocol selection;
- channel/select semantic planning;
- any other source-language decision already owned by Typed HIR or FIR.

Unsupported FIR must produce an explicit backend diagnostic. Codegen must never inspect AST, HIR, or Typed HIR to fill in missing semantics.

```text
Forge source
    -> AST
    -> HIR
    -> Typed HIR        Forge semantics
    -> FIR              final Forge semantic IR
    -> forge-fir        frozen backend-facing surface
    -> FIR -> CLIF      mechanical code-generation lowering
    -> Cranelift
         -> AArch64
         -> RISC-V64
         -> SIA         later Cranelift ISA backend
```

## Ownership below FIR

Forge owns the FIR contract and the deterministic FIR -> CLIF translation. Cranelift owns CLIF verification/optimization, instruction selection, register allocation, target ABI details, legalization and machine-code emission.

SIA will be implemented as a Cranelift target after the AArch64 and RISC-V64 paths validate the FIR -> CLIF translation. It must not become a Forge-specific backend.

Forge pins the reduced `nickik/crainlift` fork at commit `fcd03035697e5f9b68bf674698ae0da128f4f5c7`. The fork is one clean commit over the upstream snapshot and removes the Wasmtime runtime, WASI, Winch, component model, Pulley and unrelated product/tooling trees. The small `wasmtime-internal-core` utility crate remains only because current Cranelift internals directly depend on it; it is not a Wasmtime runtime dependency.

## Milestones

### C0 + C1 — boundary and crate — complete

- [x] Freeze FIR as the final Forge semantic boundary.
- [x] Add `forge-fir` as the narrow backend-facing FIR facade.
- [x] Prevent `forge-codegen-cranelift` from depending directly on `forge-frontend`.
- [x] Add `forge-codegen-cranelift` outside `forge-frontend`.
- [x] Codegen consumes `FirModule` and the FIR verifier through `forge-fir` only.
- [x] Add backend-local diagnostics.
- [x] Configure Cranelift AArch64 and RISC-V64 targets.
- [x] Add Cranelift context and target-ABI signature creation.
- [x] Accept a verified empty FIR module.
- [x] Reject non-empty unsupported FIR explicitly rather than guessing.
- [x] Pin the reduced `nickik/crainlift` fork by exact commit.
- [x] Remove unused `cranelift-frontend`/`cranelift-module` dependencies from C1.
- [x] Keep Cranelift's std feature from implicitly enabling Gimli or fuzz-control dependencies.

### C2 — type lowering

- [ ] Define canonical FIR -> CLIF scalar type mapping.
- [ ] Make pointer width a target property.
- [ ] Separate value representation from storage/layout representation.
- [ ] Reject unsupported aggregate types explicitly.

### C3 — CFG lowering

- [ ] Lower FIR functions and parameters.
- [ ] Lower blocks, constants and SSA/block values.
- [ ] Lower unconditional/conditional branches and returns.
- [ ] Run Cranelift's verifier on every generated function.

### C4 — integer operations

- [ ] Arithmetic, bit operations, shifts and comparisons.
- [ ] Signed/unsigned conversions.
- [ ] Forge checked-arithmetic control flow.

### C5 — AArch64 execution

- [ ] Compile and execute small Forge programs natively on AArch64.

### C6 — RISC-V64 execution

- [ ] Generate the same programs for RISC-V64 and execute under the test environment/emulator.
- [ ] Differentially compare observable results with AArch64.

Later milestones add memory, calls/ABI, aggregates/layout, globals, closures, runtime operations, AOT objects, optimization policy and full differential conformance. Only after those paths are mature does the SIA Cranelift backend begin.
