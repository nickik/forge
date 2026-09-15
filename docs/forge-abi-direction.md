# Forge ABI direction for C9

**Status:** accepted design direction for C9 exploration. Scalar calls are implemented by C8. Aggregate memory layout and aggregate call lowering are **not implemented or frozen yet**; C9 must validate the rules with measurements before making them normative.

## Existing SIA direction reviewed

The current SIA architecture freezes the architectural roles of `r13` as the ABI stack pointer and `r14` as the ABI link register; `r15` remains a general register with an optional ABI frame-pointer role. `BL`, `CALLR`, and `RET` already provide the control-transfer behavior needed by an ordinary compiler calling convention.

The SIA completion plan currently proposes, but has not yet frozen, this ABI register partition:

```text
r0       zero
r1-r6    arguments / returns / fast IPC message registers
r7-r8    caller-saved temporaries
r9-r12   callee-saved
r13      sp
r14      lr / caller-saved link
r15      callee-saved general register; optional frame pointer
```

The same plan leaves 64-bit values, aggregate passing, stack details, varargs, TLS, syscall/IPC ABI, trap-frame layout and unwind/debug metadata open. Forge therefore must not encode SIA physical register assignments in FIR -> CLIF. The future SIA Cranelift backend owns physical register assignment once the SIA ABI is frozen.

## C8 boundary

C8 defines only the mechanically necessary scalar call contract:

- scalar integer, `bool`, pointer, reference and function-pointer parameters;
- zero or one scalar return value;
- direct module calls;
- first-class function references;
- indirect calls through a resolved Forge function type;
- exact FIR argument/result type agreement;
- target-native Cranelift call convention for the current AArch64 and RISC-V64 validation targets.

C8 deliberately does **not** use Cranelift's C-struct `StructArgument` or `StructReturn` facilities. Those encode platform C ABI policy, not Forge aggregate policy.

Required Forge tail calls remain an explicit unsupported boundary until the target backend can guarantee a proper non-growing tail call.

## C9 design principles

Forge keeps these three contracts distinct:

```text
source aggregate
    ↓
Forge memory layout
    ↓
Forge call ABI decomposition
    ↓
target register/stack assignment
```

A value's byte representation in memory does not have to equal its decomposition at a call boundary, and neither should dictate the physical register convention of one ISA.

This separation is especially useful for SIA: Forge can minimize padding in memory, flatten values into word-sized call pieces, and let the SIA backend exploit consecutive registers and `LDP/STP` / `LD4/ST4`.

## 1. Ordinary Forge structs use optimized deterministic layout

Ordinary Forge structs should **not** inherit C's source-order layout merely because their fields were written in an order.

Because fields are named, the default Forge layout may reorder them deterministically to reduce padding. The C9 baseline to evaluate is:

1. descending required alignment;
2. descending storage size within the same alignment class;
3. declaration index as the deterministic tie-breaker.

Example:

```text
struct Example {
    a: u8;
    b: u64;
    c: u16;
    d: u32;
}
```

may use an ordinary Forge storage order equivalent to:

```text
b, d, c, a
```

rather than C-like `a, b, c, d`.

The algorithm must be deterministic for ABI-visible types. Profile-guided or build-dependent reordering is not permitted in a stable public ABI.

C9 should still provide explicit representation modes for cases where layout matters:

```text
ordinary Forge layout   deterministic optimized layout
ordered/source layout   declaration order preserved
C representation        C field layout + C ABI interoperability
packed representation   explicit reduced alignment/packing
```

The exact metadata spelling remains language/spec work.

## 2. Memory layout and call ABI are independent

A struct can have one compact in-memory representation and a different mechanical decomposition when passed or returned.

For example:

```text
struct SliceLike {
    ptr: *T;
    count: usize;
}
```

should naturally decompose into two pointer-sized ABI pieces even though it remains one Forge value semantically and may have target-specific storage details.

C9 must not classify native Forge aggregates by asking how the host C ABI classifies the corresponding struct.

## 3. Flatten aggregates into Forge ABI pieces

Aggregate call lowering should recursively flatten values into directly passable ABI pieces.

A piece is a target-independent Forge calling-unit description such as:

```text
integer word
pointer/reference word
function pointer
later: floating-point scalar
```

The target backend then assigns those pieces to registers or stack slots.

This means the Forge ABI describes a logical call decomposition while AArch64, RV64 and SIA each retain their own physical calling convention.

## 4. Use SIA's register bank aggressively

For SIA32 the natural integer ABI piece is a 32-bit word.

If the proposed `r1-r6` argument/return bank survives compiler validation, small aggregate values should be eligible for direct multi-register passing instead of immediately falling back to memory.

Candidate mapping for measurement:

```text
1 word       r1
2 words      r1:r2
3 words      r1:r3
4 words      r1:r4
5 words      r1:r5
6 words      r1:r6
larger       indirect or split according to the final ABI rule
```

SIA's `LDP/STP` and `LD4/ST4` make two- and four-word groups particularly attractive because they can efficiently spill, reload, save and restore consecutive register groups.

Do **not** freeze the direct aggregate limit yet. Both four words and six words are credible SIA thresholds:

- four words align particularly well with `LD4/ST4`;
- six words use the complete proposed argument/return register bank.

C9 should benchmark both before choosing.

## 5. Small aggregate returns should stay in registers

Forge's common systems values should not automatically require hidden return buffers.

Good candidates for direct multi-register returns include:

- slices and string views;
- small records;
- compact `Result[T,E]` values;
- compact tagged values;
- two-word and four-word machine abstractions.

The same SIA `r1-r6` bank being proposed for parameters and returns makes this straightforward in principle.

## 6. Use niches before adding explicit tags

Forge should not lower every optional or tagged value to a C-style `{ tag, payload }` record.

C9 should preserve useful niches where the representation contract permits it. In particular:

```text
Option<&T>
Option<*T> where zero is not otherwise a valid safe value
other scalar types with unused bit patterns
```

should be able to remain the size of the underlying scalar where possible.

For sum types:

- use the smallest sufficient tag;
- exploit a stable niche where one exists;
- flatten tag and payload into call ABI pieces when profitable;
- keep memory representation and call decomposition independent.

## 7. Large aggregates become indirect only at a Forge-defined threshold

For sufficiently large or awkward values, indirect passing remains appropriate.

The caller may provide storage and pass a pointer, or an equivalent Forge-defined indirect convention may be used. The threshold is a Forge target-ABI property and must not be inherited automatically from the platform C ABI.

The C9 experiments should explicitly compare:

```text
4-word direct limit
6-word direct limit on SIA
register/stack split variants
indirect-only beyond threshold
```

## 8. Register/stack splitting is allowed to remain experimental

Forge may eventually permit a value or argument stream to consume the remaining argument registers and spill only the remainder to the stack.

This can use registers better than conservative C ABIs that force a whole aggregate to memory after a classification threshold, but it makes unwind/debug/varargs rules more complex.

Therefore register/stack splitting should be benchmarked in C9 but should not be part of the first frozen ABI unless the win is clear.

## 9. Native Forge ABI and C ABI are separate

C interoperability is an explicit foreign ABI, not the definition of Forge's native ABI:

```text
Forge native
    optimized Forge layout
    + Forge aggregate decomposition
    + target Forge calling convention

extern C / repr(C)
    C field layout
    + platform C ABI
```

The C path may use Cranelift's C-oriented aggregate argument/return facilities where appropriate. Native Forge calls should not.

This allows Forge to optimize its ordinary data and call conventions without compromising FFI.

## 10. Public ABI versus internal optimization

A stable exported Forge ABI needs deterministic, versioned layout and call-decomposition rules.

Internal/LTO-private values may eventually be optimized more aggressively when the compiler has whole-program visibility, but this must never silently alter externally visible representation.

The likely split is:

```text
stable Forge ABI
    deterministic memory layout
    deterministic call decomposition

internal compiler ABI
    may optimize further when visibility proves it safe
```

The first C9 implementation should prefer the stable form first; more aggressive private ABI rewriting can come later.

## C9 experiments before freezing

C9 should gather generated-code and layout data before the ABI becomes normative:

1. Measure padding removed by deterministic field reordering across representative Forge/Cosmic structures.
2. Compare 2-, 4- and 6-word direct aggregate passing.
3. Compare 4-word versus 6-word direct returns on SIA-like workloads.
4. Measure register/stack splitting against whole-value stack fallback.
5. Determine how 64-bit values on SIA32 consume the register bank: ordinary consecutive words or aligned register pairs.
6. Identify which niche optimizations must be ABI-stable in Forge v1.
7. Verify that `repr(C)` / `extern C` stays isolated from native Forge layout and calling convention.
8. Test slices, `Option`, `Result`, small records, nested records, arrays and tagged unions independently.

## Explicit non-decisions

This branch does **not** yet freeze:

- four words versus six words as SIA's direct aggregate threshold;
- aligned-pair rules for 64-bit SIA32 values;
- register/stack splitting;
- exact representation metadata syntax;
- varargs;
- TLS;
- unwind/debug ABI;
- object/relocation format;
- the final normative SIA register ABI.

Those decisions require C9 measurements and, for physical SIA registers, a later synchronized `SIA32-ABI.md`.
