# FIR Step 16 — Boundary Hardening

Step 16 completes the FIR completion program defined in `docs/fir-completion-plan.md`.

## Contract

Successful typed HIR must cross the FIR boundary with no unresolved semantic ambiguity. The hardened crate-level `lower_fir` entry point now verifies typed HIR before lowering and verifies the complete FIR module afterwards.

### Typed-HIR boundary verification

- Function, local, expression, global, and runtime-initializer types must be concrete.
- Typed expression IDs must be unique within a body.
- `context`, `?`, `match`, and closure expressions must carry their resolved typed-HIR forms rather than generic `Source` semantics.
- Resolved call, closure, context, try, match, bit-field, and unsafe-operation nodes must match their expected HIR source shape.
- Reader forms are rejected unless they are the already-typed core `#duration "..."` form.
- Runtime-global initializer owner/result metadata is checked before lowering.

### Whole-module FIR verification

- Every global has a concrete type and matching owner.
- Every runtime initializer appears exactly once in `global_init_order`.
- Every runtime dependency exists and precedes its dependent initializer.
- Runtime initializer functions return the exact global type.
- Existing per-function FIR CFG/type verification is applied to both ordinary functions and runtime initializer functions.
- `FirInstructionKind::Poison` is forbidden in verified successful FIR.
- Every FIR value type is concrete.

## Stable dump

`dump_fir_module` emits deterministic pretty JSON because FIR uses ordered maps for module/function data. A checked-in golden locks the module-level serialization schema, while the existing focused FIR tests continue to cover the concrete operations introduced in Steps 1–15.

## Acceptance

Step 16 is complete when the existing Step 1–15 FIR tests remain green through the hardened crate-level lowering entry point, the new boundary/module-verifier tests pass, the golden dump matches, and the standard formatting/check/test/conformance/clippy gate passes.
