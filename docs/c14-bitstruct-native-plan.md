# C14 native bitstruct plan

## Goal

Make Forge v1 bitstruct construction, field reads, checked writes, and native
execution work through the production FIR-to-Cranelift path.

## Normative basis

- `docs/forge-v1-spec.md` §32;
- `docs/compiler-architecture.md` bitstruct decision: numeric storage,
  least-significant-bit declaration order, zero-extended reads, and checked
  dynamic writes.

## Current state

The first C14 checkpoint now lowers `BitStructStorage`,
`BitStructFromStorage`, and `BitFieldCheck` through the layout-backed aggregate
path. Its executable proof deliberately uses `u8` storage and exact-width
numeric fields.

The normal frontend lowering still represents two bitfield-specific operations
as general `Convert` instructions:

- a masked storage value converted to a narrower read-field type; and
- a narrow numeric write value converted to the wider declared storage type.

The generic backend verifier intentionally rejects the first as lossy. That is
correct for ordinary source conversions, but it means the current checkpoint
does not yet prove ordinary `u16`/`u32` storage or one-bit `bool` fields.

## Invariants

- The declared storage type is authoritative; no host C bitfield ABI is used.
- A bitstruct remains an aggregate at the C9 ABI boundary.
- Reads and rebuilds transfer exactly the storage representation.
- Narrow writes trap on a value outside the field width; they never truncate.

## Completed first increment

- Storage projection/rebuild uses the C9 aggregate address machinery.
- `BitFieldCheck` lowers to an unsigned comparison and Forge overflow trap.
- AArch64 native normal and range-trap fixtures pass.

## Next increment: explicit bitfield conversion semantics

### Goal

Represent extraction and insertion width changes as explicit FIR operations so
the backend can distinguish bitfield masks from source-level casts.

### Milestones

1. Add a typed FIR operation for zero-extending a masked bitfield value into
   its declared field type, and another for extending an already range-checked
   field value into declared storage.
2. Emit those operations only from bitfield read/write lowering; retain the
   generic `Convert` verifier's rejection of lossy source conversions.
3. Lower the new operations in the C14 scalar backend using CLIF integer
   extension/reduction instructions with width validation.
4. Add FIR shape tests plus native `u16` storage, narrow numeric-field,
   boolean-field, and range-trap execution fixtures.
5. Run the full hosted-native, workspace, conformance, and Clippy gates; only
   then advance the completeness matrix.

## Tests

- Existing exact-width normal and range-trap fixtures remain regression tests.
- FIR tests must assert the new bitfield-specific operations and assert that
  ordinary source conversions remain generic `Convert` values.
- Native fixtures must exercise read-modify-write, zero-extended reads, and
  a rejected out-of-range write for a narrow numeric field.

## Risks / decisions

- Do not teach generic `Convert` to truncate after a mask. It has no evidence
  that a source conversion was protected by a bitfield range check, and doing
  so would weaken ordinary Forge conversion semantics.
- Do not rely on host C bitfield layout or ABI classes. The declared storage
  type and Forge's least-significant-bit order remain authoritative.

## Completion record

Initial exact-width lowering merged as `a636725d32da3d0192be3472be57b201ec9f2ec5` after hosted-native CI. The explicit conversion increment is planned but not yet implemented.
