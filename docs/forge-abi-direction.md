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

## 4. Forge call ABI recursively decomposes values into scalar fragments

Call ABI decomposition is mechanical and independent from C struct classification.

Baseline recursive decomposition:

```text
integer scalar        -> integer fragment
bool                  -> integer fragment
pointer/reference     -> pointer fragment
function pointer      -> pointer fragment
distinct scalar       -> underlying representation fragment
struct                -> recursively decompose fields
array [T; N]          -> N repetitions of T decomposition
slice / string view   -> pointer fragment + usize fragment
zero-sized value      -> no fragments
```

Struct fields are decomposed in increasing physical field offset. Nested aggregates apply the same rule recursively.

Padding is not a fragment and never becomes an ABI piece.

Type aliases have exactly the decomposition of their aliased type. `distinct` types preserve source-level type identity but have the ABI decomposition of their underlying representation.

## 5. Compatible sub-word integer fragments are coalesced into ABI words

After recursive decomposition, Forge mechanically coalesces compatible adjacent sub-word integer fragments before register/stack assignment.

For an integer ABI word of `W` bits:

1. walk scalar fragments in decomposition order;
2. consecutive integer/bool fragments smaller than `W` may share one ABI word while their total occupied bits fit;
3. preserve each fragment's increasing-memory-offset order within the word;
4. start a new ABI word when the next fragment does not fit;
5. full-word and multiword integer fragments start at the next ABI-word boundary;
6. pointer, reference and function-pointer fragments always start their own ABI piece and are never packed together with integer fragments;
7. padding bytes are ignored rather than encoded into the ABI word.

For SIA32, `W = 32`. Thus values such as:

```text
{ u8, u8, u8, u8 } -> 1 ABI word
{ u16, u8, u8 }    -> 1 ABI word
{ u32, u8, u8 }    -> 2 ABI words
```

The packed word is a call representation only. It does not change the aggregate's in-memory representation.

On little-endian targets, the earliest fragment occupies the least-significant available bits of the ABI word. Later big-endian targets must define the corresponding deterministic mapping as part of their target ABI.

The callee reconstructs the original scalar fragments mechanically from the ABI word before ordinary FIR semantics observe them.

## 6. SIA32 uses four words as the direct single-aggregate limit

For the initial SIA32 Forge ABI design:

- one integer ABI word is 32 bits;
- the four-word limit is applied **after sub-word coalescing**;
- a single aggregate with at most four ABI words may be passed or returned directly;
- a single aggregate requiring more than four ABI words is indirect;
- the overall scalar argument bank may still use the proposed `r1-r6`; the four-word rule is a per-aggregate direct-value limit, not a six-argument limit.

This intentionally favors the natural `LD4/ST4` group size while still leaving `r5-r6` useful for additional scalar arguments.

## 7. Register/stack splitting is allowed

When a directly passed value fits the direct-value rule but there are not enough argument registers left, it may be split between the remaining argument registers and stack ABI locations.

Example:

```text
r1-r5 already occupied
next argument = 3-word direct aggregate

piece 0 -> r6
piece 1 -> stack
piece 2 -> stack
```

The value is not forced wholly to the stack merely because all of its pieces do not fit in registers.

The target ABI must define deterministic continuation order from registers to stack. For SIA32 this is ordinary increasing ABI-piece order.

## 8. SIA32 64-bit values use ordinary consecutive slots

A 64-bit integer on SIA32 decomposes into two consecutive 32-bit ABI words.

It may begin in any available argument or return slot. There is no artificial even-register alignment rule.

Piece order follows increasing memory significance. On little-endian SIA32:

```text
lower-numbered ABI slot -> low 32 bits
next ABI slot           -> high 32 bits
```

If only one register remains, the low word may occupy that register and the high word may continue on the stack under the normal splitting rule.

## 9. Indirect aggregates retain value semantics

An aggregate exceeding the direct-value limit is passed indirectly through caller-provided storage containing the value.

For an indirect parameter, the ABI behaves as if the callee receives a pointer to the argument value. For an indirect return, the caller provides return storage and the callee writes the result there through a hidden ABI argument.

The language value remains a value; the indirection is only a calling-convention representation. The compiler may eliminate temporary copies when it can prove Forge value semantics are preserved.

# Sum types and stable niches

## 10. Layout carries stable niche information

A representation may expose bit patterns that cannot represent a valid value. C9 calls these stable niches.

The initial guaranteed niche sources are:

- `bool`: canonical stored values are 0 and 1, leaving the other `u8` representations as niches;
- safe references: zero is not a valid reference representation;
- raw pointers under Forge's non-null raw-pointer semantics: zero is not an ordinary valid pointer value;
- enum discriminant representations: unused discriminant values are niches.

C9 does not derive additional pointer niches merely from alignment.

Niche choice is deterministic: when several equivalent numeric niche values are available, use the lowest available representation first.

## 11. `Option<T>` uses a stable niche when available

If `T` exposes at least one stable niche, `Option<T>` uses one niche for `None` and otherwise uses the unchanged valid representation of `T` for `Some(T)`.

Consequently common values such as:

```text
Option<&T>
Option<*T>
Option<bool>
```

need no separate tag when a stable niche is available.

Unused niches remain available to the resulting type, permitting nested optional values to remain compact while sufficient unused representations remain.

If no stable niche is available, `Option<T>` uses the ordinary explicit-tag sum representation.

## 12. Simple tagged unions may consume payload niches

A tagged union with exactly one payload-bearing variant and one or more fieldless variants may encode the fieldless variants in stable niches of the payload representation when enough niches exist.

Assignment is deterministic:

1. payload-bearing valid representations retain their normal meaning;
2. fieldless variants are assigned in source declaration order;
3. the lowest available niche representations are consumed first.

If there are not enough stable niches, use the explicit-tag representation instead.

C9 does not attempt general niche packing for multiple independent payload-bearing variants.

## 13. Explicit tags use the smallest ordinary integer storage class

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

Inactive payload storage has no source-level value. The call ABI decomposes the resulting representation mechanically and may coalesce compatible sub-word integer fragments under the normal C9 rule.

# Deferred beyond C9

The following are intentionally left for future Forge/ABI versions rather than being part of C9:

- reusing nested aggregate tail padding for outer fields or discriminants;
- alignment-derived pointer niches and other aggressive niche discovery;
- bit-packing `bool` or small integer fields in memory;
- floating-point aggregate/register classes;
- private/LTO-specific ABI rewriting and scalar replacement;
- source-order, C-compatible and packed aggregate representations;
- C interoperability;
- varargs;
- TLS;
- unwind/debug ABI;
- object/relocation format.

C9 should keep its internal layout/ABI abstractions extensible enough that these can be added later without confusing memory layout, call decomposition and physical target assignment.

# Remaining C9 implementation detail

The major aggregate design choices are now specified. The remaining target-level detail to settle during implementation is the exact stack-slot size/alignment rule for split SIA32 arguments. This should be kept in the target ABI layer rather than embedded in Forge aggregate semantics.
