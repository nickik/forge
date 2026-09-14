# FIR Completion Plan

## Goal

Finish the typed-HIR -> FIR boundary in small, independently testable increments. FIR must consume semantic decisions made above it and must not re-run type resolution or invent source-language semantics.

This plan starts from commit `d01a883` (`Implement Forge FIR lowering`). Only one numbered step is implemented at a time.

## Normative basis

- Forge v1 spec section 29: `match` preserves source arm priority and closed sums are exhaustive.
- Sections 30-31: pattern forms, guards, and irrefutable destructuring.
- Sections 33-39: positional/named calls, closures, and `Result` propagation.
- Section 32: bitstruct semantics.
- Sections 44-45: checked operations and `unsafe`.
- Section 49: narrow execution context and scoped overrides.
- Section 60: typed channels and `select`.
- Compatibility contract: pattern ordering, bounds/overflow, `unsafe`, `Result`, and `defer` semantics are frozen.

## Current state

Core FIR already provides typed values, locals/places, basic blocks, explicit terminators, direct/indirect calls, checked/wrapping arithmetic, explicit bounds checks, aggregates, Option/Result operations, loops, `defer`, and `?` propagation. Stable `ExprId`s connect HIR expressions to typed-HIR facts.

The remaining boundary gaps are intentionally diagnosed instead of guessed by FIR.

## Invariants

- Evaluate every source expression exactly once and in Forge-defined left-to-right order.
- Preserve match arm source priority and guard ordering.
- No `Ty::Unknown`, literal pseudo-types, or source-only semantic ambiguity may cross successful typed HIR -> FIR lowering.
- FIR never performs overload resolution, default-argument selection, capture inference, unsafe authorization, or collection-protocol lookup.
- `defer` cleanup edges remain correct across return, loop exits, and `?` propagation.
- New lowering machinery must remain target-independent.

## TODO / milestones

1. **Boolean match decision plans and FIR CFG lowering.** Add a typed semantic match plan for `bool` literal and wildcard arms, preserve source order and guards, and lower expression/block arms into explicit FIR branches and a join value. Do not implement other pattern families yet.
2. **Optional match decisions.** Extend semantic match plans with `None`/`Some`, payload extraction, bindings, guards, and FIR Option tests.
3. **Enum discriminant matches.** Resolve enum variant tests above FIR and lower them with explicit variant tests.
4. **Tagged-union matches.** Add discriminant tests plus typed payload-field extraction and bindings.
5. **General scalar pattern tests.** Add integer/char/string literals and integer/char ranges while preserving source priority.
6. **Structural patterns.** Add struct, fixed-array/slice sequence/rest, `as`, and `or` pattern decision nodes and binding actions.
7. **Pattern usefulness completion.** Unify exhaustiveness/unreachable analysis with the decision-tree representation so checking and lowering share one semantic model.
8. **Named/default call normalization.** Materialize all omitted `nfn` defaults and produce final parameter-order argument vectors in typed HIR.
9. **Closure environments.** Type explicit capture modes/environment fields, create non-escaping closure environment representation, and lower closure construction/calls.
10. **Execution context.** Resolve `context.<slot>` and `with context` overrides into typed slots/scoped save-restore operations before FIR.
11. **Channels and `select`.** Define the typed channel/select semantic interface, payload types, timeout type, and runtime-operation IDs; then lower select CFG.
12. **Unsafe provenance.** Track unsafe authorization in semantic HIR and require it for raw dereference/pointer operations before emitting corresponding FIR operations.
13. **Bitstruct semantics.** Materialize storage width, LSB-first field offsets, ordinary Forge field types, checked writes, and FIR mask/shift operations.
14. **Collection/map pattern protocol.** Define a concrete typed collection-pattern protocol; until then map patterns must not reach FIR with invented `Unknown` semantics.
15. **Runtime global initialization.** Separate constant/static data from runtime initializer functions and define deterministic module-init ordering/dependencies.
16. **FIR boundary hardening.** Add whole-module verification that successful semantic output contains no unresolved source-only constructs, plus FIR dump/golden tests for all completed features.

## Step 1 acceptance tests

- `match (flag) { true => a, false => b }` lowers without `fir/pattern-decision-tree-missing`.
- Wildcard fallback lowers correctly.
- Guarded bool arm falls through to later arms when its guard is false.
- Scrutinee is evaluated once.
- Both expression arms and block arms produce valid terminated CFG.
- Match expressions merge their result through a synthetic local; no SSA/phi requirement is introduced.
- Non-bool match patterns continue to report the existing FIR boundary diagnostic; later TODO steps are not implemented accidentally.
- Existing workspace tests, conformance suite, formatting, check, and clippy stay green.

## Risks / decisions

The semantic plan intentionally starts narrower than a full Rust-style pattern matrix. Rust's architecture is useful here: usefulness/type semantics are established before MIR/CFG construction, while MIR building consumes resolved pattern decisions. Forge keeps this simpler and target-independent. The first increment proves that boundary with booleans before adding payload-bearing patterns.

## Steps 2-4 acceptance tests

- Optional `None`/`Some` tests lower through `OptionIsSome`; `Some` payloads are unwrapped only on the matching edge.
- Optional payload bindings are stored before guards, so guards may reference those locals.
- Closed enum arms lower through resolved `VariantIs` tests without FIR consulting enum definitions.
- Tagged-union arms lower through resolved `VariantIs` plus typed payload-field projections and local bindings.
- Guards retain source-order fallthrough for Option, enum, and tagged-union arms.
- Structural/scalar/range/or/sequence patterns remain later milestones and still stop at the FIR boundary.

## Completion record

- Step 1 complete: typed boolean/wildcard match plans and explicit FIR CFG lowering.
- Step 2 complete: optional `None`/`Some` decisions, payload extraction, bindings, and guarded fallthrough.
- Step 3 complete: enum discriminant decisions lowered as resolved FIR variant tests.
- Step 4 complete: tagged-union discriminant tests plus typed payload-field extraction/bindings.
- Step 5 complete: integer/char/string literal tests and integer/char ranges lower from semantic match conditions.
- Step 6 complete: struct, sequence/rest, as, and OR patterns lower through typed alternatives/projections with short-circuit-safe structural tests.
- Step 7 complete: exhaustiveness and unreachable-arm usefulness for planned patterns now consume the same semantic alternatives/conditions that FIR lowers.
- Steps 8-16 intentionally untouched.
