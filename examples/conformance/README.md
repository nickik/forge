# Forge conformance examples

These programs are small, original Forge examples derived from the *kinds* of cases used by mature C compiler suites. They are not verbatim translations of upstream test bodies.

The structure mirrors useful distinctions in GCC and Clang testing:

- `run/` — programs that should eventually compile, link, run, and return `0`.
- `check/` — valid programs that must pass the production compiler's semantic
  and code-generation check without requiring a user-defined `main`.
- `parse/` — valid source examples primarily intended to exercise frontend syntax and AST construction.
- `negative/` — programs that should parse far enough for semantic analysis and then be rejected.
- `suite.fdn` — executable machine-readable metadata consumed by `forge-conformance`.

The initial set emphasizes the C-family fundamentals that routinely expose compiler bugs: precedence, fixed-width arithmetic, comparisons, bit operations, calls, recursion, aggregate layout, arrays, references, strict conversions, and type errors.

Some examples target normative Forge v1 semantics that are ahead of the current bootstrap parser or semantic analyzer. They remain here as executable specification fixtures rather than being weakened to match implementation gaps.

## Running the suite

From the repository root:

```sh
cargo run -p forge-conformance -- examples/conformance/suite.fdn
```

The manifest has an explicit `:active-kinds` vector. Parse, syntax-negative and
semantic-negative cases run through the production Rust frontend, while `run`
cases execute through the production Rust compiler on AArch64 Linux. The
normal CI lane requires every listed case to execute:

```sh
cargo run -p forge-conformance -- \
  --require-no-pending examples/conformance/suite.fdn
```

The strict flag exits unsuccessfully when any case is pending, even if all
executed cases passed. Without it, a developer can still run a suite during
bring-up on a host that lacks an active executor and inspect its pending cases.
A complete current run reports:

```text
summary: 131 passed; 0 failed; 0 pending
```

The progression rule is deliberate:

1. `[:parse]` while the source parser is the executable frontend.
2. Add `:negative` only after HIR/type checking can classify semantic rejections by the `:expect` keyword.
3. Add `:run` only after FIR plus a backend can build and execute programs and validate `:exit`.

CI's `--require-no-pending` gate prevents the manifest from claiming compiler
coverage that the production Rust path does not execute.

## Manifest contract

Each test entry has:

```fdn
{:path #path "parse/example.fg" :kind :parse}
{:path #path "check/example.fg" :kind :check}
{:path #path "negative/example.fg" :kind :negative :expect :type/mismatch}
{:path #path "run/example.fg" :kind :run :exit 0}
```

The separate `examples/spec-examples/suite.fdn` uses the same runner and adds a
required `:spec` string to each case, making its normative source mapping
explicit and reviewable.

`forge-conformance` currently implements the FDN subset needed by this manifest: maps, vectors, keywords, integers, strings, comments, optional commas, and tagged values such as `#path`. When the general FDN parser is implemented, the runner should consume that crate instead of maintaining a second parser.

## Upstream inspiration

The categories were selected after reviewing GCC's `gcc.c-torture` split between compile and execute tests, Clang's parser/diagnostic/AST/IR test styles, and the public `c-testsuite` single-file execution model. Forge tests should remain small enough that a failure identifies one language rule.

## Convention

A `run` example returns `0` on success and a small non-zero code on failure. Negative tests state the intended rejection in a leading comment. Exact diagnostic wording is not yet frozen; the semantic category is.
