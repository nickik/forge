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

The normal frontend lowering originally represented two bitfield-specific
operations as general `Convert` instructions:

- a masked storage value converted to a narrower read-field type; and
- a narrow numeric write value converted to the wider declared storage type.

The generic backend verifier intentionally rejects the first as lossy. That is
correct for ordinary source conversions. The follow-up now represents both as
explicit `BitFieldExtract` and `BitFieldExtend` FIR operations, retaining that
generic conversion boundary while proving `u16`, narrow numeric, and one-bit
`bool` fields through native execution.

## Invariants

- The declared storage type is authoritative; no host C bitfield ABI is used.
- A bitstruct remains an aggregate at the C9 ABI boundary.
- Reads and rebuilds transfer exactly the storage representation.
- Narrow writes trap on a value outside the field width; they never truncate.

## Completed first increment

- Storage projection/rebuild uses the C9 aggregate address machinery.
- `BitFieldCheck` lowers to an unsigned comparison and Forge overflow trap.
- AArch64 native normal and range-trap fixtures pass.

## Completed explicit bitfield conversion increment

The increment adds typed extraction and extension operations, emits them only
from bitfield read/write lowering, and lowers them using checked CLIF integer
reduction/extension. The native fixture covers `u16` storage, narrow numeric
and boolean fields; the trap fixture covers out-of-range writes.

## Risks / decisions

- Do not teach generic `Convert` to truncate after a mask. It has no evidence
  that a source conversion was protected by a bitfield range check, and doing
  so would weaken ordinary Forge conversion semantics.
- Do not rely on host C bitfield layout or ABI classes. The declared storage
  type and Forge's least-significant-bit order remain authoritative.

## Completion record

Initial exact-width lowering merged as `a636725d32da3d0192be3472be57b201ec9f2ec5`. Explicit conversion semantics merged as `ab9483f9bcb1bea94b3ba4082b86f1fd573c0bda` after hosted-native CI.
