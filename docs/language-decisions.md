# Forge v1 Language Decisions

This file summarizes rationale. The normative specification remains `forge-v1-spec.md`.

1. **C-shaped syntax, type-after-name declarations.** Familiar braces/operators; declarations read `x: u32`.
2. **Immutable by default.** `val` is ordinary; mutation is marked with `var`/`mut`.
3. **No null.** Absence is a type-level fact: `T?` is `Option[T]`.
4. **Strict conversions.** C numeric promotion and array decay are rejected.
5. **Production checks by default.** Bounds and ordinary overflow checks stay enabled unless explicitly removed or proven redundant.
6. **No user generics in v1.** Concrete type generation and exact overloads cover early needs without freezing a generic system prematurely.
7. **Rich tagged data.** Enums, tagged unions and advanced patterns are core.
8. **Structured metaprogramming.** `#tag` reader hooks and `@` metadata replace textual preprocessing.
9. **Result, not exceptions.** Recoverable failures are values; `panic` is fatal.
10. **Defer.** Cleanup is lexical and explicit.
11. **Closures are useful but bounded.** v1 closures may capture but may not automatically escape through hidden heap allocation.
12. **Durable allocation is explicit.** Allocator is passed directly or is part of the owning object. Temporary non-escaping memory may use `context.scratch`.
13. **Context is narrow.** Scratch memory, logging, clock/random/trace services; not arbitrary application dependency injection.
14. **OS concurrency, not a hidden scheduler.** Threads are OS threads; CSP can use OS threads/processes and explicit channels.
15. **Agents move work to data.** Standard-library agents provide serialized function/action application to owned state.
16. **Simple bootstrap compiler.** Pratt + recursive descent, typed three-address IR, local optimization, linear-scan allocation; SSA later if useful.
