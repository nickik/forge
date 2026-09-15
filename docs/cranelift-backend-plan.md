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

### C2 — type lowering — complete

- [x] Define canonical FIR -> CLIF scalar type mapping.
- [x] Make pointer width a target property.
- [x] Separate value representation from storage/layout representation.
- [x] Reject unsupported aggregate types explicitly.

### C3 — CFG lowering — complete

- [x] Lower FIR functions and parameters.
- [x] Lower blocks, constants and SSA/block values.
- [x] Lower unconditional/conditional branches and returns.
- [x] Run Cranelift's verifier on every generated function.

### C4 — integer operations — complete

- [x] Arithmetic, bit operations, shifts and comparisons.
- [x] Signed/unsigned conversions.
- [x] Forge checked-arithmetic traps from explicit FIR overflow policy.

### C5 — AArch64 execution — complete

- [x] Compile and execute small Forge programs natively on AArch64.

### C6 — RISC-V64 execution — complete

- [x] Generate the same programs for RISC-V64.
- [x] Execute emitted RISC-V64 code under the test environment/emulator.

### C7 — memory lowering — complete

- [x] Define Forge-owned scalar size/alignment facts independently of CLIF.
- [x] Lower scalar local stack storage and parameter spills.
- [x] Lower safe-reference and non-volatile raw-pointer loads/stores.
- [x] Lower address-of and scalar pointer arithmetic using Forge pointee stride.
- [x] Keep aggregate/index places and volatile semantics explicit future boundaries.

### C8 — scalar ABI and calls — complete

- [x] Centralize scalar FIR function-signature -> CLIF ABI lowering.
- [x] Lower direct calls between verified FIR module functions.
- [x] Lower first-class function references and indirect calls.
- [x] Validate exact FIR argument/result types mechanically at codegen boundary.
- [x] Preserve required-tail calls as an explicit unsupported boundary until guaranteed lowering exists.
- [x] Keep direct-call relocation resolution in the later object/linking milestone.
- [x] Do not use C-struct ABI classification for Forge native aggregates.
- [x] Record aggregate ABI/layout design direction without freezing C9.

### C9 — aggregate representation and Forge ABI/layout

- [ ] Define deterministic Forge aggregate memory layout.
- [ ] Define aggregate call decomposition independently from memory field layout.
- [ ] Decide direct-register versus indirect thresholds from AArch64/RV64/SIA evidence.
- [ ] Define explicit C-FFI representation/calling-convention escape hatch separately from native Forge ABI.

Later milestones add globals, closures, runtime operations, AOT objects, optimization policy and full differential conformance. Only after those paths are mature does the SIA Cranelift backend begin.
