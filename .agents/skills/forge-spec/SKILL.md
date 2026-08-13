# Forge Specification Work

Use this skill when changing Forge syntax, type rules, memory semantics, FDN, or compatibility guarantees.

1. Read `docs/forge-v1-spec.md`, `docs/compatibility.md`, and `docs/language-decisions.md`.
2. State whether the request changes syntax, static semantics, runtime semantics, library convention, or implementation only.
3. For language changes, update the normative spec before or with code.
4. Add at least one valid example and one invalid/counterexample.
5. Check the 40-year compatibility rule: do not repurpose existing syntax or silently change semantics.
6. Keep user generics, exceptions, language-level null, implicit numeric conversions, and textual preprocessing out of v1 unless the project owner explicitly changes the v1 charter.
