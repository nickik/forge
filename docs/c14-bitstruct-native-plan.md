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

The frontend already emits `BitStructStorage`, `BitStructFromStorage`, and
`BitFieldCheck` around normal shifts/masks. The C14 scalar lowering delegates
those operations to an older backend path that rejects them.

## Invariants

- The declared storage type is authoritative; no host C bitfield ABI is used.
- A bitstruct remains an aggregate at the C9 ABI boundary.
- Reads and rebuilds transfer exactly the storage representation.
- Narrow writes trap on a value outside the field width; they never truncate.

## Milestones

1. Lower storage projection and rebuild using the existing layout-backed
   aggregate address machinery, first proving exact-width storage/field paths.
2. Lower `BitFieldCheck` to an unsigned range comparison and Forge overflow
   trap.
3. Add a native normal-path fixture and a range-trap fixture.
4. Run the full AArch64 native specification and workspace gates; update the
   C14 matrix only on green evidence.

## Completion record

Pending CI validation.
