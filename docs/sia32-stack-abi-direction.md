# SIA32 Forge stack ABI direction

**Status:** proposed C9 rule for split and stack-passed Forge ABI pieces. This is a Forge-side design proposal until synchronized with a future normative `SIA32-ABI.md`.

The SIA plan already calls out 16-byte public-call stack alignment as an ABI item to freeze. Forge's aggregate ABI is word-oriented on SIA32, so the simplest stack rule is to make stack argument locations a continuation of the same 32-bit ABI-piece stream used by the argument registers.

## Proposed rule

### ABI stack slot

Every non-zero integer/pointer ABI piece that reaches the stack occupies exactly one SIA32 ABI stack slot:

```text
slot size       4 bytes
slot alignment  4 bytes
```

There is no additional 8-byte alignment for `u64`/`i64`, no aggregate-boundary alignment, and no holes inserted between ABI pieces.

A 64-bit value therefore remains two ordinary consecutive 32-bit pieces on the stack, exactly as it does in the register piece stream.

### Public-call stack alignment

At every public call instruction, `sp` is 16-byte aligned.

The caller reserves an outgoing stack-argument area whose size is:

```text
round_up(number_of_stack_words * 4, 16)
```

Only the actual ABI words carry values. Any bytes added solely to reach 16-byte call alignment are trailing padding and do not create ABI locations.

### Placement

SIA uses a downward-growing stack for the proposed convention.

After reserving the outgoing area, stack ABI words are stored at increasing addresses from the call-time stack pointer:

```text
stack piece 0 -> [sp + 0]
stack piece 1 -> [sp + 4]
stack piece 2 -> [sp + 8]
...
```

Here `sp` means the value of the stack pointer at the call instruction and therefore the callee's entry stack pointer before its own prologue changes it.

Because the link is held in `r14`, the call instruction does not implicitly push a return address between `sp` and the incoming stack arguments.

### Register/stack continuation

Arguments are processed in source argument order. Each argument is first decomposed into its Forge ABI pieces, including sub-word coalescing. Direct pieces consume the available argument registers in ABI-piece order. Once the register bank is exhausted, the remaining pieces continue in the stack word stream without reclassification or realignment.

With the proposed `r1-r6` argument bank:

```text
r1-r5 already occupied
next argument = [piece0, piece1, piece2]

piece0 -> r6
piece1 -> [sp + 0]
piece2 -> [sp + 4]
```

There is no rule that moves the whole aggregate to the stack merely because only part of it fits in registers.

### Multiword order

Piece order follows increasing memory significance/order already chosen by the Forge ABI decomposition.

For little-endian SIA32, a 64-bit scalar uses:

```text
first ABI piece   low 32 bits
second ABI piece  high 32 bits
```

If split at the register boundary, for example with only `r6` available:

```text
r6       low 32 bits
[sp + 0] high 32 bits
```

No even-register or 8-byte stack alignment is required.

### Sub-word scalar values

A standalone integer/bool ABI piece smaller than 32 bits still consumes one 4-byte stack slot when stack-passed. Its value uses the same canonical extension expected for the corresponding register representation:

- unsigned integers are zero-extended to 32 bits;
- signed integers are sign-extended to 32 bits;
- `bool` is canonical 0 or 1.

Sub-word fragments inside aggregates are first subject to the C9 coalescing rule; a coalesced 32-bit word then occupies one ordinary stack slot.

### Indirect values

An aggregate that exceeds the four-word direct-value limit is represented by its indirect pointer for call placement. On SIA32 that pointer is one ordinary 32-bit ABI piece and therefore follows the same register/stack assignment rules.

The same principle should eventually apply to hidden return-storage pointers once their exact position in the physical SIA calling convention is frozen.

### Ownership and restoration

The caller owns the outgoing argument area. After the call returns, the caller restores `sp` by the amount reserved for the call.

The callee may establish its own frame, but references to incoming stack arguments are defined relative to the callee's entry stack pointer, not its post-prologue `sp`.

## Why this rule

This avoids recreating C-style stack classification after Forge has already decomposed values into ABI pieces.

Advantages:

- one uniform 32-bit word model across registers and stack;
- split aggregates require no special case;
- `u64` can cross the register/stack boundary naturally;
- no wasted holes for pair alignment;
- simple caller and callee lowering;
- simple debugger/unwind description later because stack arguments form a dense word array;
- 16-byte public-call alignment remains available for efficient four-word transfers and future implementation requirements.

The intentional trade-off is that a stack-passed `u8`/`u16` consumes a full 32-bit slot. That cost is small, predictable, and preferable to introducing byte-granular stack packing into the call ABI.

## Examples

Two stack words:

```text
raw stack argument bytes = 8
outgoing area            = 16

[sp + 0]  word 0
[sp + 4]  word 1
[sp + 8]  alignment padding
[sp + 12] alignment padding
```

Five stack words:

```text
raw stack argument bytes = 20
outgoing area            = 32

[sp + 0]  word 0
[sp + 4]  word 1
[sp + 8]  word 2
[sp + 12] word 3
[sp + 16] word 4
[sp + 20..31] alignment padding
```

Split 64-bit value with only one argument register left:

```text
r6       low word
[sp + 0] high word
```

## Recommendation

Adopt this as the C9 implementation rule unless SIA hardware/compiler experiments expose a concrete disadvantage:

```text
SIA32 stack ABI = dense 4-byte word stream
call-time sp     = 16-byte aligned
outgoing size    = round_up(stack_words * 4, 16)
padding          = trailing only
u64 alignment    = no special rule
split values     = continue directly from registers to stack
```

This keeps Forge's logical ABI and SIA's eventual physical ABI easy to map while preserving the four-word aggregate design and unrestricted register/stack splitting already selected for C9.
