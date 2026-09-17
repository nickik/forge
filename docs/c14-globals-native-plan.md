# C14 native globals plan

## Goal

Make mutable Forge globals and taking their shared or mutable address work
through the normal FIR-to-Cranelift object and hosted-native execution path.

## Normative basis

- `docs/forge-v1-spec.md` declaration and mutability rules;
- `docs/compiler-architecture.md` FIR and target-boundary rules.

## Current state

Global values have static storage, relocations, reads, and ordered runtime
initialization. The first C14 increment proved mutable scalar assignment and
address taking on hosted AArch64 (`69187bc`). This follow-up uses the existing
memory-value representation to extend the same explicit FIR operations to
aggregate values, and adds negative object-boundary relocation coverage.

## Invariants

- Only source `var` globals are writable or may have a mutable address taken.
- `const` and `val` globals remain immutable after initialization.
- Global symbol addresses are emitted by the existing object/relocation path;
  no Forge-specific runtime storage is introduced.
- Aggregate globals retain the existing C9 layout and are copied byte-for-byte
  between their temporary aggregate backing storage and their symbol address.
- Static pointers must fail object emission if their function or global target
  is not present in the object plan; no partially relocated object is emitted.

## Milestones

1. Retain the explicit FIR global store and address-of operations introduced
   for scalar globals.
2. Use `store_typed_value` for aggregate stores so global destinations obey the
   same layout/copy rules as local and reference destinations.
3. Prove aggregate initialization, whole-value replacement, readback and
   reference-based field mutation in a native AArch64 fixture.
4. Prove positive static pointer relocations and reject missing function/global
   relocation targets for both AArch64 and RISC-V object emission.
5. Lower direct field places rooted in a global through `AddressOfGlobal` plus
   the existing reference projection path; retain immutable-root diagnostics.
6. Serialize function and global addresses inside a static aggregate, then
   prove their relocations link and execute on both native object targets.
7. Update the C14 matrix and roadmap only after the full native CI gate is
   green.

## Tests

- A `var` aggregate global is initialized, read, replaced as a whole and
  updated through `&mut` across a function call.
- Existing static global/function pointer relocations link and execute.
- Missing static function and global relocation targets are rejected before
  object emission for both native object targets.
- A read-only static aggregate contains both a function pointer and a global
  pointer, with each relocated address observed by the AArch64/RISC-V harness.
- Assignment to a `val` global and `&mut` of an immutable global are rejected.
- FIR shape tests assert the dedicated global operations for scalar and
  aggregate globals.

## Risks / decisions

Do not encode a global as a synthetic local or weaken ordinary place rules.
Dedicated FIR operations retain global symbol identity for object emission and
make immutable-global violations diagnosable at the boundary. Global-rooted
field places are expressed as `AddressOfGlobal` plus ordinary reference
projection rather than as a duplicate place kind.

## Completion record

Scalar global mutation/address taking merged as `69187bc`. Aggregate global
initialization, whole-value replacement, reads, and mutation through a mutable
address are covered by `global_aggregate.fg`. Static function/global pointer
relocations continue to link and execute; missing targets are rejected before
object emission on both AArch64 and RISC-V. The implementation head
`bb98e58dbdff4a5a9c2301e419fc2357ad979c5b` passed hosted-native CI run
`35209849435` (job `105164491122`). Direct global-field reads, stores, and
mutable field addresses lower through `AddressOfGlobal` plus normal projection;
their focused frontend tests and the hosted AArch64 fixture passed on
`36cdea3ef50950ce58af013c305935117bb30cd7` in CI run `35211049973` (job
`105168369552`). Static aggregate pointer relocation execution is pending its
focused object and native-harness proof.
