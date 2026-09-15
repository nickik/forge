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

### C9 — aggregate representation and native Forge ABI — complete

#### C9a — Forge layout engine — complete

- [x] Make `forge-fir` own the single authoritative Forge memory-layout engine.
- [x] Preserve nominal field/variant declaration indices across the FIR boundary.
- [x] Define deterministic default struct field reordering and zero-sized-field placement.
- [x] Define nested structs, arrays, field offsets, ZSTs and recursive-by-value cycle rejection.
- [x] Define stable initial niches, `Option<T>`, enums, tagged unions and `Result` layouts.
- [x] Select the smallest explicit discriminant width when a niche cannot represent the sum.
- [x] Make the older C7 scalar layout helper delegate to the authoritative layout engine.

#### C9b — ABI decomposition — complete

- [x] Keep Forge ABI decomposition independent from both memory field ordering and Cranelift ABI classification.
- [x] Recursively flatten aggregate values into Forge ABI fragments and pieces.
- [x] Coalesce compatible sub-word integer/bool fragments into ABI words.
- [x] Keep pointers/references/function pointers as independently classified pieces.
- [x] Exclude memory padding from ABI pieces.
- [x] Freeze the initial direct-value threshold at four target ABI words; larger values are indirect.
- [x] Define ordinary consecutive-word decomposition for multiword integers with no artificial pair alignment.

#### C9c — FIR aggregate operations — complete

- [x] Lower aggregate construction and aggregate local storage/copies.
- [x] Lower field extraction and field-address places from authoritative Forge layout offsets.
- [x] Lower array construction, indexing, length and bounds checks.
- [x] Lower aggregate pointer arithmetic using authoritative aggregate stride.
- [x] Lower initial niche/tag encode, test and payload operations for `Option`, `Result` and tagged values.
- [x] Keep aggregate values internal to codegen as compiler-owned memory temporaries rather than inventing CLIF aggregate types.

#### C9d — aggregate calls and returns — complete

- [x] Derive CLIF signatures mechanically from Forge `AbiDecomposition`.
- [x] Pack/unpack direct aggregate parameters into their scalar ABI pieces.
- [x] Support direct multi-piece aggregate returns.
- [x] Pass values larger than four ABI words indirectly while preserving Forge value semantics in the callee.
- [x] Use a hidden first pointer argument for large aggregate return storage without adopting Cranelift's C-struct ABI classification.
- [x] Apply the same Forge ABI to direct calls, indirect calls and first-class function references.
- [x] Add cross-target aggregate call round-trip lowering tests for direct, indirect and function-pointer cases.

C interop, packed/source-ordered representations, broader niche use, floating-point aggregate classes, tail-padding reuse and private/LTO ABI rewriting remain explicitly deferred to future ABI versions; see `docs/forge-abi-future.md`.

Later milestones add globals, closures, runtime operations, AOT objects, optimization policy and full differential conformance. Only after those paths are mature does the SIA Cranelift backend begin.
