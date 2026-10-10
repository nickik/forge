# Checked explicit integer conversions

## Goal

Make every explicit conversion between Forge integer types usable in native code while preserving values: conversions trap when the source value is outside the target type's range and never silently truncate or reinterpret bits.

## Normative basis

- `docs/forge-v1-spec.md` §22 (strict conversions) and §24 (defined arithmetic traps).
- `docs/forge-v1-spec.md` §23 (boolean equality versus ordered comparisons).
- `docs/runtime-abi.md` (checked traps converge on the canonical non-returning panic hook).

The proposed normative rule is: an explicit integer-to-integer conversion succeeds only when the mathematical value is representable by the target type; otherwise it follows the defined arithmetic trap/panic path. Explicit conversion syntax does not request wrapping or bit reinterpretation.

## Current state

Typed HIR lowers explicit casts into `FirInstructionKind::Convert { value, target }`. FIR already preserves source and target integer types. The Cranelift backend accepted identity and lossless widening only, and rejected narrowing/signedness-changing conversions because their policy was previously unspecified. Cosmic's ext2/file-service code uses explicit masked integer casts for little-endian byte serialization, so the backend rejection prevented those graphs from compiling. The same service graph also exposed missing boolean equality/inequality lowering for bitmap state checks.

## Invariants

- No implicit numeric conversion is introduced.
- Existing lossless widening remains trap-free.
- Narrowing and signedness changes are range-checked before representation normalization.
- Casts never silently truncate or reinterpret bit patterns.
- Non-integer conversions and pointer conversions retain their existing semantics and safety checks.
- Checked failures use the existing integer-overflow trap class.

## Milestones

1. Document the explicit cast rule in the language spec and conversion matrix.
2. Lower all integer casts with source-domain range checks followed by the existing sign/zero extension or reduction.
3. Add AArch64 and RV64 lowering coverage for narrowing, same-width signedness changes, signed-to-unsigned, unsigned-to-signed, and lossless widening.
4. Run compiler tests and use Forge's check/build path on Cosmic's filesystem service and tests to reveal any subsequent concrete blocker.
5. Lower boolean equality and inequality as comparisons over canonical boolean values, so the file-service graph is supported without treating booleans as integers.

## Tests

- Positive CLIF tests assert checks for out-of-range conversions and no range check for fully contained widening.
- Cover signed and unsigned values, including byte-sized types and pointer-sized integers under both supported target layouts.
- Existing C4 arithmetic, frontend, and workspace tests remain green.
- Compile Cosmic filesystem service/test graphs and continue only from actual diagnostics.
- Cover boolean equality and inequality; reject ordered boolean comparisons.

## Risks / decisions

- Checked casts may trap where a programmer previously expected C-style truncation; such truncation is intentionally excluded, and users can mask/shift explicitly before casting when they want a byte extraction.
- The existing FIR node already distinguishes explicit casts from implicit type equality and retains both types, so a new FIR enum/policy field is unnecessary for this rule.
- Forge v1 panic is non-unwinding; cast failure follows that established runtime contract.

## Completion record

Implemented checked explicit integer casts in FIR-to-CLIF, boolean equality/inequality lowering, and a type-check diagnostic for ordered boolean comparisons. Validation passed: `cargo fmt --all -- --check`, `cargo test --workspace`, `cargo build --workspace`, and `git diff --check`. The C4b matrix covers every integer source/target pair, including pointer-sized types, on both AArch64 and RV64. The 22-module Cosmic ext2 service test graph and 85-module application roundtrip dependency graph both pass `forgec --check`. Native M31 service execution remains unverified because the pinned CForge checkout (`Cosmic/.cforge`) is absent; QEMU executors are also unavailable in this environment.
