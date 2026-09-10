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
17. **`internal` is reserved only.** Module-private remains the default; `internal` is not valid v1 source syntax.
18. **No uninitialized bindings.** `val`, `var`, and `const` declarations all require an initializer.
19. **Assignment is a statement only.** `x = value;` is legal; assignment cannot appear as an expression.
20. **No compound assignment in v1.** `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=`, and `>>=` are rejected. Write `x = x + y;` explicitly.
21. **Control-flow parentheses are mandatory.** `if`, `while`, and both forms of `for` use parenthesized headers.
22. **`switch` is absent from v1.** Use `match`.
23. **`match` is an expression.** It may appear anywhere an expression is accepted; using it as a statement is simply an expression statement.
24. **Patterns are deliberately bounded.** v1 includes OR patterns and `name @ pattern`, but not pattern conjunction `p1 & p2` or pattern negation `!pattern`.
25. **Map-pattern optionality is explicit.** `:email email?` means the key may be absent and the binding is optional; `..` ignores remaining entries.
26. **Capture-free closures omit brackets.** Write `(x: u32) -> u32 { ... }`; `[](...)` is rejected. Capturing closures use `[capture](...)`.
27. **Built-in `Option` constructors are short.** `None` and `Some(value)` do not require an `Option::` prefix.
28. **Unit variants accept either spelling.** `None`/`None{}` and `Shape::Point`/`Shape::Point{}` are both legal; formatters should prefer the bare form.
29. **Enum variants are qualified.** Write `Color::Red`; unqualified `Red` is not resolved as an enum variant.
30. **Tagged construction uses `Type::Variant{...}`.** This is the v1 constructor form.
31. **Method-call syntax is supported.** `p.length()` is source sugar resolved statically from `impl` methods.
32. **Postfix metadata is general syntax.** Expressions and types may carry `@metadata`; semantic validation decides which annotations are meaningful at each location.
33. **`@check` does not exist.** Ordinary arithmetic is already checked. `@wrap(...)` is the explicit wrapping form.
34. **`with context` uses named Forge arguments.** Example: `with context(:scratch = &test_scratch, :logger = &test_logger) { ... }`. Its values are Forge expressions, not FDN.
35. **No C ABI syntax in v1.** `extern "C"` blocks and C varargs are reserved for a later version.
36. **No generic/type-application call syntax in v1.** Forms such as `foo[T](x)` are rejected. Compiler intrinsics use ordinary calls plus contextual typing where necessary; type-taking intrinsics may accept a type as a dedicated syntactic operand such as `size_of(Point)`.
