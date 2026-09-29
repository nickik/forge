# Forge v1 specification examples

This corpus turns selected valid examples from the normative Forge v1 language
surface into executable acceptance cases. Every manifest entry identifies the
specification sections it represents and chooses the strongest applicable
stage:

- `:parse` for source forms whose runtime semantics are outside the case;
- `:check` for complete semantic and code-generation validation through the
  production Rust compiler without requiring a source `main`;
- `:run` for native execution with a documented exit result;
- `:syntax-negative` for examples the specification explicitly identifies as
  invalid or reserved v1 source. Its `:expect` keyword must match the stable
  parser diagnostic code for that rejection category; an unrelated parse
  failure does not satisfy the case.

Run the strict suite from the repository root:

```sh
cargo run -p forge-conformance --locked -- \
  --require-no-pending examples/spec-examples/suite.fdn
```

The corpus grows alongside reconciliation of `docs/forge-v1-spec.md`. A valid
specification example is not considered covered merely because a similar parser
fixture exists; it must be listed here with an explicit `:spec` mapping.
