# Forge ABI direction after C8

**Status:** design direction only. Scalar calls are implemented by C8. Aggregate memory layout and aggregate call ABI are intentionally **not frozen** here; that is C9 work.

## Existing SIA direction reviewed for C8

The current SIA architecture freezes the architectural roles of `r13` as the ABI stack pointer and `r14` as the ABI link register; `r15` remains a general register with an optional ABI frame-pointer role. `BL`, `CALLR`, and `RET` already use the architectural link behavior needed by an ordinary compiler calling convention.

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

The same plan explicitly leaves 64-bit values, aggregate passing, stack details, varargs, TLS, syscall/IPC ABI, trap-frame layout and unwind/debug metadata open. Consequently C8 does not encode SIA physical register assignments in Forge FIR -> CLIF. The future SIA Cranelift backend should implement the physical register convention once the SIA ABI is frozen.

## C8 ABI boundary

C8 defines only the mechanically necessary scalar call contract:

- scalar integer, `bool`, pointer, reference and function-pointer parameters;
- zero or one scalar return value;
- direct module calls;
- first-class function references;
- indirect calls through a resolved Forge function type;
- exact FIR argument/result type agreement;
- target-native Cranelift call convention for the current AArch64 and RISC-V64 validation targets.

C8 deliberately does **not** use Cranelift's C-struct `StructArgument` or `StructReturn` facilities. Those encode platform C ABI policy, not Forge aggregate policy.

Required Forge tail calls remain an explicit unsupported boundary until we can guarantee the target backend emits a proper non-growing tail call. Ordinary non-required tail-call optimization remains an optimization concern.

## Separate three concepts

Forge should keep these distinct:

```text
source aggregate
    ↓
Forge memory layout
    ↓
Forge call ABI decomposition
    ↓
target register/stack assignment
```

A struct's byte layout in memory does not have to be identical to how that value is decomposed into argument or return registers.

This separation is especially useful for SIA: memory layout can minimize padding while the call ABI can exploit consecutive word registers and the SIA pair/quad transfer instructions.

## Candidate Forge memory-layout policy for C9

### Default named-field reordering

Unlike C, Forge fields are accessed by name and ordinary Forge representation need not preserve declaration order. A strong default candidate is deterministic field reordering to minimize padding:

1. descending required alignment;
2. descending storage size within an alignment class;
3. declaration index as the deterministic tie-breaker.

For example, a source type conceptually written as:

```text
struct Example {
    a: u8;
    b: u64;
    c: u16;
    d: u32;
}
```

need not use the C-like order `a,b,c,d`. Ordinary Forge layout could use the equivalent of `b,d,c,a`, substantially reducing padding on targets where `u64` has eight-byte alignment.

This must be a specified deterministic algorithm for ABI-visible types. Profile-guided or build-dependent field reordering should not be part of a stable public ABI.

### Explicit representation modes

The eventual layout design should probably distinguish at least:

- ordinary Forge layout: deterministic optimized layout;
- source-order/ordered layout: declaration order preserved when binary layout matters;
- C representation: opt-in FFI layout and C calling convention;
- packed representation: explicit reduced-alignment/packing semantics.

Exact metadata spelling is C9 language/spec work and is not chosen here.

## Candidate aggregate call ABI

### Flatten values into ABI pieces

Rather than asking whether a struct is a C struct, recursively classify a value into Forge ABI pieces. A piece is a directly passable scalar unit such as an integer word, pointer or later floating-point unit.

For example:

```text
struct Pair {
    ptr: &T;
    count: usize;
}
```

could be represented to the call ABI as two pointer-sized pieces even though it remains one Forge value semantically.

This makes the ABI independent of C struct-classification rules.

### SIA word-oriented register passing

For SIA32 the natural ABI piece is a 32-bit word. If the proposed `r1-r6` argument/return bank survives compiler validation, small aggregate values could occupy several consecutive argument/return words rather than immediately falling back to memory.

Potential direction:

```text
1 word       r1
2 words      r1:r2
3 words      r1:r3
4 words      r1:r4
5-6 words    r1:r5 / r1:r6 when profitable
larger       indirect or split according to the frozen ABI rule
```

This aligns unusually well with SIA's `LDP/STP` and `LD4/ST4` instructions, which efficiently move two or four consecutive registers to/from memory.

The exact direct-value threshold should be selected from generated-code measurements, not frozen in C8.

### Permit register/stack splitting

A Forge ABI could permit an aggregate or argument stream to consume the remaining argument registers and spill only the remainder to the stack. That uses registers better than many conservative C ABI rules that force whole aggregates to memory after a classification threshold.

This is attractive but increases unwind/debug and varargs complexity, so it needs measurement before adoption.

### Prefer multi-register returns for small results

Forge's `Result`, slices and small records are common systems-language values. Returning two or several ABI pieces directly can avoid caller-allocated return buffers and hidden memory traffic.

The SIA proposal already reserves the same `r1-r6` bank for arguments and returns, making multi-word returns straightforward in principle. Again, the exact maximum is intentionally not frozen yet.

## Sum types and niches

Forge should not lower every `Option` or tagged value to a C-like `{ tag, payload }` record.

Candidate rules include:

- preserve pointer/reference niches so `Option<&T>` can stay one word when valid;
- choose the smallest sufficient tag representation;
- flatten tag plus payload into ABI pieces when that is cheaper than indirect passing;
- permit specialized packed/sidecar representations where the type/layout contract explicitly allows them.

Memory representation and call decomposition should remain separate so a packed in-memory sum can still be expanded into convenient register pieces at a call boundary.

## Large aggregates

For sufficiently large or awkward aggregates, indirect passing remains sensible. The caller can provide storage and pass a pointer, or an equivalent Forge-defined indirect convention can be used.

The threshold should be a Forge target-ABI property rather than inherited from the host C ABI. On SIA, four words and six words are both plausible breakpoints worth benchmarking because the ISA has efficient four-register transfers and the proposed ABI has six argument/return registers.

## Public ABI versus internal optimization

A stable exported Forge ABI must have deterministic, versioned layout and call-decomposition rules. Internal/LTO-private types can eventually permit more aggressive transformations, but those optimizations must never silently change externally visible representation.

This suggests two useful contracts:

```text
stable Forge ABI
    deterministic layout + call decomposition

internal compiler ABI
    may optimize further when whole-program visibility proves it safe
```

C9 should decide whether the initial implementation exposes both or deliberately implements only the stable form first.

## FFI

C interoperability should be explicit rather than defining Forge's native ABI:

```text
Forge native call
    Forge layout + Forge ABI

C FFI call
    C-compatible representation + platform C ABI
```

The C path may use Cranelift's C-oriented struct argument/return facilities where appropriate. Native Forge calls should not.

## Questions C9 should answer with experiments

Before freezing aggregate layout/passing:

1. How much padding is removed by deterministic alignment-class field reordering on representative Forge/Cosmic structures?
2. What direct aggregate size gives best code on AArch64, RV64 and the planned SIA32 backend?
3. Is six-word SIA return passing actually better than a four-word limit plus indirect return?
4. Is register/stack splitting worth the implementation/debugging complexity?
5. How should 64-bit SIA32 scalar values consume the proposed register bank: consecutive aligned pairs or ordinary consecutive slots?
6. Which niche optimizations must be ABI-stable from Forge v1 versus internal optimizations?
7. What explicit representation metadata is needed for FFI, hardware structures, disk/network formats and mmap'd data?

C8 intentionally leaves these questions open while establishing enough scalar call machinery to measure them later.
