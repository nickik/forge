# C9 integration TODO

Goal: integrate the complete C9 Forge ABI/layout milestone onto the current C8 backend in one reviewed branch.

- [x] C9a — authoritative Forge layout engine
  - memory size/alignment/field order owned by Forge, not CLIF/C ABI
  - nominal structs/enums/tagged types, arrays/slices, option/result layout
  - stable niche handling and recursion/error boundaries
  - preserve scalar C7/C8 behavior through the new layout engine

- [x] C9b — Forge ABI decomposition
  - decompose aggregate representations into Forge ABI pieces
  - support SIA32/native64 target models and direct/indirect threshold
  - coalesce sub-word fragments deterministically
  - keep floating-point ABI classes and unsupported semantic forms explicit

- [x] C9c — aggregate FIR memory/value lowering
  - aggregate locals and stack storage
  - aggregate construction/extraction and field/index places
  - option/result/tagged operations driven only by Forge layout metadata
  - preserve mutability, scheduler and exact FIR type invariants from C7/C8

- [x] C9d — aggregate calls and returns
  - direct aggregate parameters/returns via Forge ABI pieces
  - indirect aggregate parameters and hidden return storage
  - direct and indirect calls use exact Forge function types
  - relocation boundary remains explicit

- [ ] Combined review/test sweep
  - rebase old C9 semantics onto current C8 rather than replacing hardened code
  - port old tests, then add regression tests for layout determinism, direct/indirect ABI boundaries, aggregate call shape, mutability and non-topological dependencies
  - formatting, frontend, workspace, native AArch64, RV64/QEMU and conformance have passed together
  - run final strict workspace CI through Clippy
  - squash integration history to one clean C9 commit
  - fast-forward `main` only after the combined branch is green
