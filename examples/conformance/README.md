# Forge conformance examples

These programs are small, original Forge examples derived from the *kinds* of cases used by mature C compiler suites. They are not verbatim translations of upstream test bodies.

The structure mirrors useful distinctions in GCC and Clang testing:

- `run/` — programs that should eventually compile, link, run, and return `0`.
- `parse/` — valid source examples primarily intended to exercise frontend syntax and AST construction.
- `negative/` — programs that should parse far enough for semantic analysis and then be rejected.
- `suite.fdn` — machine-readable test metadata for a future Forge conformance runner.

The initial set emphasizes the C-family fundamentals that routinely expose compiler bugs: precedence, fixed-width arithmetic, comparisons, bit operations, calls, recursion, aggregate layout, arrays, references, strict conversions, and type errors.

Some examples target normative Forge v1 semantics that are ahead of the current bootstrap parser or semantic analyzer. They are intentionally kept here as executable specification fixtures rather than weakened to match implementation gaps.

## Upstream inspiration

The categories were selected after reviewing GCC's `gcc.c-torture` split between compile and execute tests, Clang's parser/diagnostic/AST/IR test styles, and the public `c-testsuite` single-file execution model. Forge tests should remain small enough that a failure identifies one language rule.

## Convention

A `run` example returns `0` on success and a small non-zero code on failure. Negative tests state the intended rejection in a leading comment. Exact diagnostic wording is not yet frozen; the semantic category is.
