# C14 native mutable globals plan

## Goal

Make mutable scalar Forge globals and taking their shared or mutable address
work through the normal FIR-to-Cranelift object and hosted-native execution
path.

## Normative basis

- `docs/forge-v1-spec.md` declaration and mutability rules;
- `docs/compiler-architecture.md` FIR and target-boundary rules.

## Current state

Global values already have static storage, relocations, reads, and ordered
runtime initialization. FIR represents a global read, but mutable global
assignment and address taking cannot cross the FIR boundary as distinct,
verifiable operations.

## Invariants

- Only source `var` globals are writable or may have a mutable address taken.
- `const` and `val` globals remain immutable after initialization.
- Global symbol addresses are emitted by the existing object/relocation path;
  no Forge-specific runtime storage is introduced.
- This increment is scalar-only. Aggregate storage and address relocations are
  a follow-up increment.

## Milestones

1. Add explicit FIR operations for scalar global store and address-of.
2. Preserve mutability at the FIR global boundary and validate it in backend
   lowering.
3. Lower those operations via the existing symbolic global address mechanism.
4. Prove mutation across calls and reference-based mutation in a native AArch64
   fixture, plus focused FIR/type diagnostics.
5. Update the C14 matrix and roadmap only after the full native CI gate is
   green.

## Tests

- A `var` scalar global is read and updated by multiple functions.
- `&mut` of a mutable global is passed to a function that mutates it.
- Assignment to a `val` global and `&mut` of an immutable global are rejected.
- FIR shape tests assert the dedicated global operations.

## Risks / decisions

Do not encode a global as a synthetic local or weaken ordinary place rules.
Dedicated FIR operations retain global symbol identity for object emission and
make immutable-global violations diagnosable at the boundary.

## Completion record

Pending implementation and hosted-native CI.
