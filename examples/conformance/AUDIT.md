# Forge v1 parser conformance audit

This suite is intentionally split into executable categories:

- `parse/`: valid Forge v1 source that must produce an AST without diagnostics.
- `syntax-negative/`: source that must be rejected by the lexer/parser.
- `negative/`: semantically invalid source, activated when HIR/name/type checking exists.
- `run/`: execution tests, activated when code generation exists.

The parser audit should grow by adding a positive fixture for every accepted grammar family and one or more negative fixtures for nearby reserved, malformed, or deliberately excluded forms. A parser change is not considered complete until the relevant fixtures are active in `suite.fdn` and CI passes.

Current focus areas include FDN/metadata boundaries, declarations and defaults, call argument modes, control-flow headers, patterns, closures, postfix expressions, lexical errors, and reserved future syntax.
