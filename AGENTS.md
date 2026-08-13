# AGENTS.md

## Mission

Implement Forge v1 exactly enough that programs written to the normative spec remain valid as the compiler evolves. Prefer simple, auditable compiler machinery over clever infrastructure.

## Read before changing code

1. `docs/index.md`
2. The relevant normative sections in `docs/forge-v1-spec.md` or `docs/fdn-v1-spec.md`
3. `docs/compiler-architecture.md`
4. `docs/compatibility.md`

For work expected to span multiple subsystems or more than one focused coding session, create/update an execution plan following `PLANS.md` before implementation.

## Non-negotiable v1 rules

- `val` immutable; `var` mutable.
- Declarations use type-after-name syntax: `x: u32`.
- No language-level `null`; `T?` means `Option[T]`.
- No user-defined generics in v1.
- No implicit numeric promotion or signed/unsigned mixing.
- Bounds and ordinary integer-overflow checks are enabled by default in production semantics unless locally made unchecked/explicitly wrapping.
- Unsafe raw-memory operations require `unsafe`.
- `Result[T,E]` + `?` is recoverable error handling; no language exceptions.
- Durable/escaping allocation is explicit through an allocator or owning object; non-escaping temporary allocation may use `context.scratch`.
- Reader tags and metadata are structured; never add a C-style textual preprocessor.
- New language syntax requires a specification change and tests.

## Compiler direction

- Rust frontend uses Logos 0.16 for lexing and Chumsky 0.13 for parsing/recovery.
- Preserve a source-faithful spanned AST; semantic lowering must not happen in the parser.
- Typed three-address IR first; do not introduce SSA into the frontend.
- Optimize simply: constant folding, local value numbering, copy propagation, DCE, liveness, linear-scan allocation, target peepholes.
- Keep target-independent semantics separate from backend details.

## Change discipline

- Add focused tests for every parser/type-system/semantic change.
- Prefer deterministic diagnostics and deterministic output.
- Do not silently widen accepted syntax.
- Do not add dependencies unless they materially reduce project risk.
- Do not use exceptions in bootstrap compiler code for ordinary control flow.
- Run build and tests before declaring work complete.
- Review against `CODE_REVIEW.md`.

## Useful commands

```sh
cargo test --workspace
cargo run -p forge-frontend --bin forge-parse -- --json examples/hello.fg
```
