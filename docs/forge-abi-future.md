# Forge ABI future directions

**Status:** deferred design ideas for future Forge ABI revisions. None of the items in this document are part of the C9 baseline unless promoted by a later design decision.

The C9 baseline intentionally keeps the native Forge ABI simple enough to implement and validate. The following optimizations and representation modes are worth revisiting later.

## Tail-padding reuse

Nested aggregates may contain tail padding introduced by alignment. A future Forge layout could permit an outer field, or a sum-type discriminant, to occupy otherwise-unused tail padding of a nested aggregate.

Potential benefit:

- smaller nested records and tagged values;
- better cache density;
- fewer otherwise-unused bytes.

Costs and open questions:

- aggregate copy semantics become more subtle;
- layout reasoning and debugging become harder;
- changing an inner type can affect storage belonging conceptually to an outer type;
- interactions with references to subobjects need precise rules.

Do not assume tail padding is reusable in the C9 layout.

## Alignment-derived pointer niches

C9 only relies on simple stable niches such as null references/pointers, canonical `bool` values, and unused enum discriminants.

A future ABI may exploit pointer alignment. For example, if a valid pointer to `T` must be 4-byte aligned, low-bit patterns `01`, `10`, and `11` cannot be naturally aligned pointers and could potentially encode extra enum states.

This enables compact tagged pointers and richer sum types, but should not become ABI-visible until pointer validity/provenance rules are explicit enough to make such encodings stable.

## Bit-packed bool and small fields

A future memory-layout mode may pack multiple `bool` fields, or sufficiently small integer fields, into individual bits or sub-byte ranges.

Potential benefit:

- smaller records;
- better density for large arrays of small records.

Costs:

- fields stop being independently byte-addressable;
- taking references to fields becomes harder or impossible without proxy semantics;
- mutation requires read/modify/write operations;
- atomicity rules become more complicated.

The C9 baseline keeps scalar fields byte-addressable and does not bit-pack them in memory.

## Floating-point aggregate register classes

The initial C9 aggregate ABI is focused on integer, pointer, and reference pieces.

A future ABI should distinguish floating-point ABI pieces so targets with dedicated floating-point argument/return registers can keep homogeneous or mixed floating-point aggregates out of integer registers where beneficial.

The ABI model should therefore remain extensible to piece classes such as:

```text
Integer
Pointer
Float
Vector
```

without redefining the semantic aggregate model.

## Private/LTO ABI rewriting

The stable Forge ABI must remain deterministic for externally visible functions and values.

Whole-program compilation may later use a private/internal ABI for functions whose complete call graph is known. Possible optimizations include:

- scalar replacement of aggregates;
- eliminating hidden indirect-return storage;
- passing only fields that are actually used;
- changing private aggregate decomposition;
- eliminating temporary aggregate construction;
- specializing call conventions between known caller/callee pairs.

These are compiler optimizations, not public ABI guarantees.

## C, packed, and source-ordered representations

C9 supports only the default Forge representation.

Future language versions may add explicit representations for interoperability or binary-layout-sensitive work:

- C-compatible/source ABI representation;
- source/declaration-ordered representation;
- packed/reduced-alignment representation.

These should remain explicit opt-in modes. They must not constrain the optimized default Forge layout.

C interoperability is intentionally not a C9 priority.

## Promotion rule

An item in this document becomes part of the Forge ABI only after it has:

1. precise representation and call-semantics rules;
2. implementation tests;
3. target-specific validation where relevant;
4. an explicit ABI-versioning decision.

Until then, these items are reserved future directions rather than promises of representation or behavior.
