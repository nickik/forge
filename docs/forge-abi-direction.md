# Forge ABI direction for C9

**Status:** accepted C9 design rules. Scalar calls are implemented by C8. Aggregate lowering is not implemented yet. The rules below are the baseline C9 implementation should target; the final physical SIA register ABI remains a separate SIA specification.

## Existing SIA direction

The current SIA architecture freezes `r13` as the ABI stack pointer and `r14` as the ABI link register; `r15` remains a general register with an optional ABI frame-pointer role. `BL`, `CALLR`, and `RET` provide the required control-transfer behavior.

The current SIA ABI direction is:

```text
r0       zero
r1-r6    arguments / returns / fast IPC message registers
r7-r8    caller-saved temporaries
r9-r12   callee-saved
r13      sp
r14      lr / caller-saved link
r15      callee-saved general register; optional frame pointer
```

This physical register partition is not yet normative SIA ABI. Forge therefore describes logical ABI pieces; the future SIA Cranelift backend maps those pieces to physical registers and stack locations.

## C8 boundary

C8 supports scalar integer, `bool`, pointer, reference and function-pointer calls, direct calls, first-class function references and indirect calls. It deliberately does not use Cranelift's C-struct ABI facilities for native Forge values.

Required Forge tail calls remain an explicit unsupported boundary until a target backend can guarantee a non-growing tail call.

# C9 accepted rules

## 1. Keep memory layout, call decomposition and register assignment separate

These are three different contracts:

```text
Forge semantic value
    ↓
Forge memory layout
    ↓
Forge call ABI decomposition
    ↓
target register / stack assignment
```

A value's byte layout does not dictate how it is decomposed for a call, and Forge call decomposition does not encode one target's physical register convention.

## 2. Default Forge aggregate layout is deterministic and optimized

C9 supports exactly one aggregate representation: the default Forge representation.

Default Forge structs may reorder named fields to reduce padding. The storage order is determined recursively and deterministically by:

1. descending required alignment;
2. descending storage size within the same alignment class;
3. source declaration index as the tie-breaker.

Example:

```text
struct Example {
    a: u8;
    b: u64;
    c: u16;
    d: u32;
}
```

uses storage order equivalent to:

```text
b, d, c, a
```

rather than source order.

For each field in that order:

1. round the current offset up to the field alignment;
2. assign that offset to the field;
3. advance by the field storage size.

Aggregate alignment is the maximum alignment of its non-zero-sized fields, or 1 if there are none.

Aggregate size is the final occupied offset rounded up to aggregate alignment. An aggregate containing only zero-sized fields therefore has size 0 and alignment 1.

The layout algorithm is deterministic. Profile-guided, build-dependent or optimization-level-dependent field ordering is not permitted for the default Forge representation.

No alternative source-order, C-compatible or packed representation is supported by C9. If unsupported representation metadata is requested, compilation must fail rather than silently changing or ignoring the request.

## 3. Zero-sized fields

A zero-sized field:

- consumes zero storage bytes;
- has placement alignment 1;
- does not increase aggregate size or alignment;
- is placed after all non-zero-sized fields;
- is ordered relative to other zero-sized fields by declaration index.

No additional address-identity rule is specified for zero-sized fields in C9.

A zero-sized function parameter decomposes to zero ABI pieces and consumes no argument register or stack slot. A zero-sized return value similarly consumes no return ABI location.

## 4. Forge call ABI recursively decomposes values into ABI pieces

Call ABI decomposition is mechanical and independent from C struct classification.

Baseline recursive decomposition:

```text
integer scalar        -> integer piece
bool                  -> scalar piece
pointer/reference     -> pointer piece
function pointer      -> pointer piece
distinct scalar       -> underlying representation piece
struct                -> recursively decompose fields
array [T; N]          -> N repetitions of T decomposition
slice / string view   -> pointer piece + usize piece
zero-sized value      -> no pieces
```

Struct fields are decomposed in increasing physical field offset so register order naturally follows storage order. Nested aggregates apply the same rule recursively.

Padding is not an ABI piece and does not consume a register or stack slot.

Type aliases have exactly the decomposition of their aliased type. `distinct` types preserve their source-level type identity but have the ABI decomposition of their underlying representation.

## 5. SIA32 uses four words as the direct single-aggregate limit

For the initial SIA32 Forge ABI design:

- one integer ABI word is 32 bits;
- a single aggregate with at most four ABI words may be passed or returned directly;
- a single aggregate requiring more than four ABI words is indirect;
- the overall scalar argument bank may still use the proposed `r1-r6`; the four-word rule is a per-aggregate direct-value limit, not a six-argument limit.

This intentionally favors the natural `LD4/ST4` group size while still leaving `r5-r6` useful for additional scalar arguments.

The logical direct aggregate return bank therefore uses at most four words even if the final SIA ABI retains six general argument/return registers.

## 6. Register/stack splitting is allowed

When a directly passed value fits the direct-value rule but there are not enough argument registers left, it may be split between the remaining argument registers and stack ABI locations.

Example direction:

```text
r1-r5 already occupied
next argument = 3-word direct aggregate

piece 0 -> r6
piece 1 -> stack
piece 2 -> stack
```

The value is not forced wholly to the stack merely because all of its pieces do not fit in registers.

The target ABI must define the deterministic continuation order from registers to stack. For SIA32 this should be ordinary increasing ABI-piece order.

## 7. SIA32 64-bit values use ordinary consecutive slots

A 64-bit integer on SIA32 decomposes into two consecutive 32-bit ABI words.

It may begin in any available argument or return slot. There is no artificial even-register alignment rule.

Piece order follows increasing memory offset. On little-endian SIA32 this means:

```text
lower-numbered ABI slot -> low 32 bits
next ABI slot           -> high 32 bits
```

If only one register remains, the low word may occupy that register and the high word may continue on the stack under the normal splitting rule.

## 8. Indirect aggregates remain value semantics

An aggregate exceeding the direct-value limit is passed indirectly through caller-provided storage containing the value.

For an indirect parameter, the ABI behaves as if the callee receives a pointer to the argument value. The language value remains a value; the indirection is only a calling convention representation.

For an indirect return, the caller provides return storage and the callee writes the result there through a hidden ABI argument.

The compiler may eliminate temporary copies when it can prove that doing so preserves Forge value semantics.

The physical placement of hidden indirect pointers belongs to each target ABI, not FIR semantics.

# Sum types and stable niches

## 9. Layout carries stable niche information

A representation may expose bit patterns that cannot represent a valid value. C9 calls these stable niches.

The initial guaranteed niche sources are:

- `bool`: canonical stored values are 0 and 1, leaving the other `u8` representations as niches;
- safe references: zero is not a valid reference representation;
- raw pointers under Forge's non-null raw-pointer semantics: zero is not an ordinary valid pointer value;
- enum discriminant representations: unused discriminant values are niches.

C9 does not derive additional pointer niches merely from alignment. Alignment-derived tagged-pointer encodings remain a possible later optimization.

Niche choice is deterministic: when several equivalent numeric niche values are available, use the lowest available representation first.

## 10. `Option<T>` uses a stable niche when available

If `T` exposes at least one stable niche, `Option<T>` uses one niche for `None` and otherwise uses the unchanged valid representation of `T` for `Some(T)`.

Consequently common values such as:

```text
Option<&T>
Option<*T>
Option<bool>
```

need no separate tag when a stable niche is available.

Unused niches remain available to the resulting type. This permits nested optional values to remain compact when sufficient unused representations remain.

If no stable niche is available, `Option<T>` uses the ordinary explicit-tag sum representation described below.

## 11. Simple tagged unions may consume payload niches

A tagged union with exactly one payload-bearing variant and one or more fieldless variants may encode the fieldless variants in stable niches of the payload representation when enough niches exist.

Assignment is deterministic:

1. payload-bearing valid representations retain their normal meaning;
2. fieldless variants are assigned in source declaration order;
3. the lowest available niche representations are consumed first.

If there are not enough stable niches, use the explicit-tag representation instead.

C9 does not attempt general niche packing for multiple independent payload-bearing variants.

## 12. Explicit tags use the smallest ordinary integer storage class

When a sum type cannot use a stable niche, it uses an explicit discriminant plus payload storage.

The discriminant uses the smallest of:

```text
u8
u16
u32
u64
```

that can encode every variant.

The payload storage has the maximum size and alignment needed by any variant payload. The tag and payload storage participate in the normal deterministic Forge aggregate-layout algorithm.

Inactive payload storage has no source-level value.

The call ABI decomposes the resulting representation mechanically; it does not replace it with a C enum ABI.

# Important optimization opportunities deliberately not frozen yet

The accepted rules above are enough to implement a correct first C9 layout engine, but several worthwhile optimizations should be evaluated before declaring the ABI permanently frozen.

## A. Sub-word ABI coalescing

Naive recursive scalar decomposition can waste registers. For example:

```text
struct FourBytes {
    a: u8;
    b: u8;
    c: u8;
    d: u8;
}
```

should not necessarily consume four 32-bit SIA argument registers.

A later ABI-classification pass could coalesce compatible adjacent sub-word scalar pieces into a word-sized ABI piece. This is probably the most important remaining call-ABI optimization to measure before freeze.

The design must decide whether coalescing follows contiguous memory bytes, a canonical field-bit stream, or another deterministic rule. Pointer/reference pieces should not be silently packed together with integer fragments.

## B. Reusing aggregate padding

Forge could eventually place outer fields or sum-type discriminants into otherwise-unused tail padding of nested aggregates.

This can reduce size beyond simple field reordering, especially for nested records and tagged values, but it complicates aggregate copying and address/layout reasoning. C9 should not require it for the first implementation.

## C. More aggressive niches

Potential later niches include alignment-invalid pointer bit patterns and other type-specific invalid states.

These can make tagged pointers and richer sums extremely compact, but they should become ABI-visible only after their validity/provenance rules are unambiguous. C9 initially uses only the stable niches listed above.

## D. Bit-packing bools and very small fields

Multiple `bool` or small integer fields could be bit-packed in memory. That saves space but makes field access, mutation and references substantially more complicated.

Do not implement this in the initial C9 default layout. Keep byte-addressable scalar fields for now.

## E. Floating-point register classes

Forge's eventual native ABI should be able to classify floating-point pieces separately on targets with FP argument registers rather than forcing all aggregates through integer words.

C9's first aggregate work can remain integer/pointer focused, but the ABI model should not assume that all future pieces belong to one register class.

## F. Private/LTO ABI rewriting

The stable ABI remains deterministic. Whole-program compilation may later choose different private call decompositions, eliminate indirect temporaries, scalar-replace aggregates or reorder private representations when no externally visible contract is affected.

This is an optimization layer, not part of the stable C9 ABI.

# Remaining C9 questions before final freeze

The design is now substantially narrower. The main remaining ABI questions are:

1. Whether and how to coalesce sub-word scalar pieces into word-sized call pieces.
2. Exact target stack-slot layout/alignment for split arguments.
3. Whether tail-padding reuse is worthwhile enough to become part of the stable default layout.
4. Whether any stable niches beyond null pointers/references, canonical `bool`, and unused enum discriminants should be guaranteed in v1.
5. Later floating-point piece classification.

C interoperability, packed representations, varargs, TLS, unwind/debug ABI and object/relocation format are outside the current C9 focus.
