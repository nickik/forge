# Forge implementation tasks

## Current milestone: Rust frontend

1. **Compile and stabilize the initial Rust parser**
   - Install a current Rust toolchain.
   - Run `cargo test --workspace`.
   - Resolve any API differences against Chumsky 0.13 / Logos 0.16 without changing Forge syntax.
   - Make `examples/hello.fg` and `examples/parser-demo.fg` parse cleanly.

2. **Complete expression and statement grammar**
   - assignment and compound assignment;
   - member/method syntax as distinct AST nodes;
   - `for`;
   - `switch`;
   - closure syntax and capture lists.

3. **Implement Forge patterns**
   - wildcard and binding patterns;
   - struct/tagged destructuring;
   - sequence/rest destructuring;
   - or/and/not patterns;
   - guards;
   - `match`;
   - recovery tests.

4. **Integrate FDN**
   - FDN value AST;
   - `@` metadata;
   - `#tag` reader forms;
   - built-in reader tags;
   - preserving unknown namespaced reader tags.

5. **Build HIR lowering**
   - declaration IDs and scopes;
   - module/import resolution;
   - named-call normalization;
   - basic source desugaring;
   - reader-form expansion boundary.

6. **Build type checking / typed HIR**
   - strict numeric conversions;
   - `T?` as `Option[T]`;
   - `Result`;
   - exact overload resolution;
   - closure captures;
   - `unsafe` checks.

7. **Build FIR**
   - typed three-address basic-block IR;
   - explicit bounds/overflow checks;
   - `Result`/`Option` operations;
   - defer cleanup edges;
   - textual FIR dump for tests.

8. **First backend**
   - Prefer LLVM IR emission once FIR is stable; alternatively use a portable C backend for differential testing.

See `docs/parser-status.md` and `docs/frontend-ir.md`.
