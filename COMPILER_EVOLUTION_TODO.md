# Forge compiler evolution TODO

## 2026-09 M27 checkpoint

- [x] Forge emits and links the freestanding SIA32 image used by Cosmic's native M27 boot.
- [x] SIA32 privileged operations used by Cosmic lower through production Cranelift: SREAD/SWRITE, TLBFENCE, SYNC.I and TRAP.
- [x] SIA32 direct-call literals preserve the required 4-byte function alignment through final image layout.
- [x] The merged Forge image is consumed by LightingSimulation's end-to-end Cosmic boot proof.
- [x] M27 proof reaches Cosmic, installs VMCTX/page tables, enables translation, enters and returns from TRAP 0x27, and halts intentionally.
- [ ] Next vertical target: compile the smallest System Task/userspace image and support the kernel/user crossing required to run it.


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
  - [x] Pin `select` terminators as a deliberate C14 backend boundary on both
        hosted targets; frontend/FIR scaffolding does not imply codegen support.
  - [x] Reject closure-valued call arguments with a stable `closure/escape`
        diagnostic; local closure calls and capture-free function pointers remain supported.
  - [x] Pin required direct, indirect and local-closure tail calls as explicit
        hosted backend boundaries; they must not silently become ordinary calls.
  - [x] Pin SIA32 scratch swap and context return as explicit CLIF-bridge
        boundaries until a Cosmic vertical slice requires their implementation.
- [x] Continue rejecting compiler-internal sentinel types at the FIR boundary.
      The verified FIR module rejects every semantic sentinel with stable
      function, block, instruction and value context before code generation.

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
- [x] `defer`: direct and `?` early return, single and nested-loop exits, and
      nested LIFO ordering execute natively.
- [x] `match`: resolved map-protocol lookup/closed-key calls execute natively,
      including required and optional bindings.
- [x] `match`: mixed fieldless, narrow-scalar and multi-field tagged payloads
      execute across argument and return ABI boundaries.
- [x] distinct types and aliases: explicit conversion and value ABI execution.
- [x] value `for`: array/slice iteration, explicit FIR control flow,
      non-iterable/refutable-binding diagnostics and native break/continue execution.

## 3. Harden ABI and object correctness

- [x] Add cross-target ABI fixtures for mixed scalar/aggregate calls, hidden
      returns, recursion and indirect calls.
- [x] Define and execute the AArch64 floating-point aggregate ABI class before
      enabling float aggregate calls; RISC-V and SIA remain explicit boundaries.
- [x] Audit object sections, relocations, visibility and deterministic symbol
      naming for every supported target.
  - [x] AArch64 and RISC-V ELF objects pin deterministic sections, symbol
        names/bindings, explicit imports/exports/locals and text/data
        relocations; SIAO32 separately pins its sections, symbols and `ABS32`
        relocation contract.
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

- [x] Complete `f32`/`f64` call/return ABI, aggregate ABI and exceptional-value
      execution matrices.
  - [x] AArch64 hosted execution covers unordered `NaN` comparisons, signed
        zero through division, and integer-to-`f64` conversion.
  - [x] AArch64 mixed-`f32`/`f64` aggregate arguments and returns execute
        through the production ABI.
  - [x] Explicit `f32`↔`f64` FIR promotion/demotion executes natively without
        weakening generic integer conversion rules.
- [x] Keep hosted executable fixtures as the acceptance proof, not object
      emission alone.

### RISC-V 64

- [ ] Maintain object and QEMU execution parity for completed scalar,
      aggregate, global and runtime-initialization features.
  - [x] Execute `duration` arguments, local load/store and return values through
        production RISC-V machine code under QEMU.
  - [x] Execute `char` arguments, local load/store and return values through
        production RISC-V machine code under QEMU.
  - [x] Execute `bool` plus signed/unsigned `8`/`16`/`32`-bit arguments,
        local load/store and return values through RISC-V machine code under QEMU.
  - [x] Execute signed `64`-bit arguments, local load/store, return values and
        signed comparison through RISC-V machine code under QEMU.
  - [x] Execute signed/unsigned pointer-width arguments, local load/store,
        return values and ordered comparisons through RISC-V machine code under QEMU.
- [ ] Add float only when the RISC-V ISA/ABI configuration and execution
      contract are intentionally selected and tested.

### SIA32 — C15 and later

- [x] Complete the integer/privileged production lowering required by the merged Cosmic M27 native boot path.
- [x] Keep fixed-GPR syscall selectors within the architectural `r0..r15`
      register file at both source-to-FIR and backend FIR validation boundaries.
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
- [x] Improve FIR verifier diagnostics so invalid producer contracts identify
      the function, block, instruction and value involved.
  - [x] Value-definition, Poison and non-concrete instruction diagnostics carry
        function/block/instruction/value context while retaining source spans
        and stable verifier codes.
  - [x] Control-flow terminator diagnostics carry function/block/value context
        plus expected/actual type or operation facts.
  - [x] Function signatures, locals and closures identify their owning function
        and expose the invalid entry/type facts.
  - [x] Whole-CFG must-initialization rejects local access unless every incoming
        control-flow path initializes it, including branch joins and loops.
- [ ] Keep lowering phases one-way: parser/HIR/typechecking decisions must be
      represented in FIR rather than reconstructed in codegen.
  - [x] Represent ordinary integer conversions as explicitly lossless FIR;
        target-aware narrowing and signedness loss stay rejected.
- [x] Add compiler debug dumps for typed HIR, FIR, ABI decomposition, CLIF and
      object plans behind stable, testable flags.
  - [x] `forgec --dump-fir` emits deterministic verified FIR JSON for the
        semantically linked root and explicit library mappings.
  - [x] `forgec --dump-typed-hir` emits deterministic resolved typed-HIR JSON
        for the same semantically linked compilation unit.
  - [x] `forgec --dump-abi` emits deterministic AArch64 parameter/return
        decomposition, including direct pieces and indirect aggregate passing.
  - [x] `forgec --dump-clif` emits deterministic production AArch64 CLIF after
        FIR verification and ABI lowering, before machine-code/object emission.
  - [x] `forgec --dump-object-plan` emits deterministic production AArch64
        function/global symbols, linkage, layouts and initializer ordering
        before section construction, relocation encoding or serialization.

## 6. Build, package and Cosmic readiness

- [ ] Finish freestanding `:kernel` validation: symbols, sections, relocations,
      entry contract and rejection of hosted dependencies.
  - [x] AArch64 Forge kernel objects export the manifest-selected entry under
        its exact platform symbol and verify ELF sections, call relocations,
        and the absence of undefined hosted/runtime imports.
  - [x] Repeat the contract with the representative Cosmic M27 early-kernel SIA32 image and execute it through LightingSimulation.
  - [x] Route explicitly mapped freestanding library units through Forge's semantic module linker before SIA32 raw/user-image emission, preserving visibility and dependency-cycle diagnostics.
  - [x] Compile the checked-in M28.5 System Task source through the production
        SIA32 user-image path, including fixed `r1` syscall exchange, traps and
        the userspace proof-word store.
  - [ ] Extend the contract to a separate freestanding System Task/user image and kernel/user ABI crossing.
- [x] Keep package/module visibility and dependency ordering exercised through
      real multi-package builds.
  - [x] Production compilation accepts transitive public dependencies
        independent of supplied library order and rejects private transitive
        access and dependency cycles before object emission.
- [x] Define the Forge-to-Cosmic kernel/userland compile contract once the
      target-independent freestanding object path is stable.
  - [x] Pin semantic library mappings, the explicit SIA32 user entry/base,
        deterministic headerless output and the boundary between image emission
        and Lighting execution with CLI acceptance tests.
- [ ] Validate each claimed Forge backend against one representative Cosmic
      component only after its ordinary compiler gate is green.

## Explicitly deferred

- SIA32 floating point before C15 is complete.
- `select` / channels, escaping closures and tail calls until they receive a
  separate language and runtime milestone.
- A second SIA machine-code backend: Forge continues to use Cranelift SIA32.
