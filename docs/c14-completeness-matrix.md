# C14 Forge v1 completeness matrix

Status at branch `agent/c14-full-language-compatibility`. This is a living audit of the
normative surface in `forge-v1-spec.md`, as corrected by
`forge-v1-syntax-decisions.md`.

Legend:

1. fully implemented and executable
2. frontend-only
3. FIR exists; native lowering incomplete
4. native lowering exists; executable coverage incomplete
5. partial or semantic mismatch
6. intentionally outside C14
7. stale/non-normative syntax

| Language family | Status | Evidence / remaining work |
| --- | ---: | --- |
| Modules, imports, visibility | 1 | Parser, resolver, multi-library compiler and build tests. |
| `val`, `var`, `const`, assignment | 1 | Typecheck/FIR/native integer corpus; definite initialization still needs a dedicated whole-CFG audit. |
| Integer, bool, byte scalars | 1 | Checked/wrapping arithmetic, comparisons, shifts, conversions and ABI tests. |
| `char` | 1 | Native constants, locals, comparison, argument and return fixture. |
| `duration` | 1 | Source builtin and reader form lower as signed nanoseconds; native argument, return, local, field load and field store execution is covered. |
| `f32`, `f64` | 4 | Native AArch64 scalar constants, arithmetic, negation, comparisons, integer-to-float conversion, `f32`/`f64` argument-return calls, aggregate field storage, and executable fixtures are implemented. Unordered `NaN` comparisons, signed zero through division, integer-to-`f64` conversion, and explicit `f32`↔`f64` promotion/demotion execute in the hosted acceptance lane. Mixed-`f32`/`f64` aggregate arguments and returns execute through the production ABI. The AArch64 native matrix is complete; SIA32 deliberately rejects float FIR until C15, and RISC-V float support remains unclaimed. |
| Structs and enums | 1 | Construction, projection, layout, ABI and matching tests. |
| Tagged unions | 1 | Construction, payload extraction, nested match, and mixed fieldless/narrow-scalar/multi-field payload argument-return ABI execute. |
| `Option[T]` | 1 | `None`, explicit `Some(value)`, implicit promotion, patterns and native layout/lowering. |
| `Result[T,E]` | 1 | Canonical `Ok=0`/`Err=1` layout semantics, contextual and payloadless constructors, nested patterns, `?`, FIR discriminant/payload lowering, AArch64 object emission, and hosted native execution are covered. |
| Arrays | 1 | Construction, indexing, bounds, aggregate elements and ABI execute. |
| Slices | 1 | Explicit array-reference views, mutable indexing, aggregate-contained views, sequence-rest lowering, and pointer/length argument/return ABI execute natively. |
| References | 1 | Shared/mutable local rules, dereference, projections and ABI covered. |
| Raw pointers and volatile | 1 | Unsafe authorization, casts, arithmetic, dereference, volatile load/store and barriers covered on AArch64/RISC-V structurally. |
| Distinct types and aliases | 1 | Nominal non-mixing diagnostics, explicit conversion, transparent aliases, and value argument/return ABI execute natively. |
| Bitstructs | 1 | Native storage projection/rebuild, checked writes, explicit field extract/extend, narrow numeric and boolean fields, and `u8`/`u16`/`u32`/`u64` value ABI execute. |
| `if`, `while`, C-style `for` | 1 | Typed CFG/FIR and executable corpus. |
| value `for` iteration | 1 | Arrays and slices type-check through an explicit built-in iteration contract, lower to `Len`/`IndexUnchecked` CFG, and execute natively with `break`/`continue`; non-iterables and refutable bindings are rejected. General user-defined iterator protocols remain future work and are not claimed here. |
| `break`, `continue` | 1 | Single and nested-loop transfers execute with scope-correct deferred cleanup on continue and break paths. |
| `match` | 1 | Bool, scalar, enum, tagged, Option, Result, nested projections, guards, OR/as, ranges, sequence-rest, and resolved map-protocol required/optional bindings execute natively. |
| Functions and calls | 1 | Direct/indirect, named/default, method calls, scalar/aggregate ABI and non-main entry covered. |
| Function pointers | 1 | Named functions cross call boundaries; anonymous closure coercion is rejected. |
| Local captured closures | 1 | Explicit capture lists and local calls execute. Escaping/cross-function closure ABI is intentionally not part of C14. |
| `defer` | 1 | Normal/direct return and `?` cleanup, break/continue exits, nested LIFO ordering, and cleanup-body control-flow rejection are covered. |
| Globals | 1 | Static data, function/global pointer relocations, ordered runtime initialization, mutable scalar/whole-aggregate stores, shared/mutable addresses, direct global-rooted field reads/stores/addresses, and static aggregate pointers execute. AArch64 native and RISC-V/QEMU object tests cover the relocation paths. |
| Overflow and traps | 1 | Checked/wrapping add/sub/mul, div/rem, shifts and divide-by-zero coverage exists. Ordinary integer conversions use dedicated lossless FIR semantics; narrowing and signed-to-unsigned conversions remain rejected until a future operation defines their policy. |
| FDN readers and metadata | 4 | Parse/preservation and duration boundary tests exist; executable behavior is provider/tool-specific. |
| Hosted providers/build system | 4 | Build/check/run/test, entries and hosted providers exist. Full current Cosmic/CKV acceptance remains. |
| Freestanding `:kernel`, `:std false` | 4 | AArch64 object emission exports the manifest-selected entry under its exact platform symbol and verifies ELF sections, call relocations, and no undefined hosted/runtime imports. The representative Cosmic M27 SIA32 kernel image executes through LightingSimulation; explicit semantic library mappings and the checked-in M28.5 `r1` syscall/proof source reach production SIA32 user-image emission. The separate System Task kernel/user crossing still lacks Lighting execution. |
| `select` / channels | 6 | Explicitly deferred by the C14 acceptance request. Existing frontend/FIR scaffolding is not completion. |
| SIA machine-code backend | 6 | C15. |
| Tail calls | 6 | Not a C14 requirement. |
| `switch`, `internal`, compound assignment | 7 | Reserved/rejected by the syntax decisions. |
| Pattern conjunction/negation | 7 | Removed from normative v1. |
| `extern "C"`, C varargs | 7 | Deferred from v1 by the syntax decisions. |
| User-defined generics | 7 | Not Forge v1. |

## Mechanical incomplete-code audit

- Backend `UnsupportedInstruction` boundaries are concentrated around genuine FIR operations;
  each must be matched against the table before removal.
- Scalar `TypeLowering` now maps `f32`, `f64`, `char`, and `duration`, but that alone does not
  prove instruction lowering or ABI execution.
- `LoadGlobal` has real object/relocation coverage; comments describing it as future work must be
  reviewed for staleness.
- `select` occurrences are intentionally retained but excluded from C14 completion.
- Generic `panic!` occurrences in compiler tests/runtime generation are not automatically language
  gaps; production fallible paths remain subject to the Rust-quality audit.

## Next acceptance slices

1. Compile a separate Cosmic System Task image and prove the kernel/user crossing through LightingSimulation.
2. Validate current Cosmic through M18 or later.
