# Forge v1 parser conformance audit

This suite is intentionally split into executable categories:

- `parse/`: valid Forge v1 source that must produce an AST without diagnostics.
- `syntax-negative/`: source that must be rejected by the lexer/parser.
- `negative/`: semantically invalid source that must reach and be rejected by the appropriate semantic stage.
- `run/`: source that must compile and execute with the declared exit status.

The parser audit should grow by adding a positive fixture for every accepted grammar family and one or more negative fixtures for nearby reserved, malformed, or deliberately excluded forms. A parser or semantic change is not considered complete until the relevant fixtures are active in `suite.fdn` and CI passes.

Current active coverage includes 35 positive parse fixtures, 67 syntax-negative fixtures, 19 semantic-negative fixtures, and 10 executable conformance fixtures. The semantic-negative set includes declaration-default type errors, non-exhaustive closed matches, and statically unreachable match arms; these expectations are executed by the type-checking stage rather than retained as pending bookkeeping.

Current focus areas include FDN/metadata boundaries, declarations and defaults, call argument modes, control-flow headers, patterns, closures, postfix expressions, lexical errors, `select`, `impl`, reserved future syntax, and native execution coverage for the integer-only Forge v1 surface.
