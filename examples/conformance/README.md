# Forge conformance examples

These programs are small, original Forge examples derived from the *kinds* of cases used by mature C compiler suites. They are not verbatim translations of upstream test bodies.

The structure mirrors useful distinctions in GCC and Clang testing:

- `run/` — programs that should eventually compile, link, run, and return `0`.
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

The manifest has an explicit `:active-kinds` vector. Initially it contains only `:parse`. Every active parse case is passed through the same `forge_frontend::parse_source` entry point used by the `forge-parse` CLI. The harness prints inactive `:negative` and `:run` cases as `PENDING` and exits successfully when all active cases pass.

A typical bring-up summary is:

```text
summary: 4 passed; 0 failed; 16 pending
```

The progression rule is deliberate:

1. `[:parse]` while the source parser is the executable frontend.
2. Add `:negative` only after HIR/type checking can classify semantic rejections by the `:expect` keyword.
3. Add `:run` only after FIR plus a backend can build and execute programs and validate `:exit`.

If a kind is placed in `:active-kinds` before its executor exists, the harness fails. This prevents the manifest from claiming compiler coverage that is not implemented.

## Manifest contract

Each test entry has:

```fdn
{:path #path "parse/example.fg" :kind :parse}
{:path #path "negative/example.fg" :kind :negative :expect :type/mismatch}
{:path #path "run/example.fg" :kind :run :exit 0}
```

`forge-conformance` currently implements the FDN subset needed by this manifest: maps, vectors, keywords, integers, strings, comments, optional commas, and tagged values such as `#path`. When the general FDN parser is implemented, the runner should consume that crate instead of maintaining a second parser.

## Upstream inspiration

The categories were selected after reviewing GCC's `gcc.c-torture` split between compile and execute tests, Clang's parser/diagnostic/AST/IR test styles, and the public `c-testsuite` single-file execution model. Forge tests should remain small enough that a failure identifies one language rule.

## Convention

A `run` example returns `0` on success and a small non-zero code on failure. Negative tests state the intended rejection in a leading comment. Exact diagnostic wording is not yet frozen; the semantic category is.
