# Parser technology decision

## Choice

The bootstrap frontend uses **Logos 0.16.1** for lexical analysis and **Chumsky 0.13.0** for the token grammar.

This split is intentional:

- Logos owns byte-oriented token recognition and source ranges.
- Chumsky owns recursive grammar composition, precedence, diagnostics/recovery, and AST construction.
- Forge compiler code owns the AST and every semantic representation after it.

The parser libraries are implementation details. Forge source syntax is defined by the Forge specification, not by library behavior.

## Why Chumsky

Forge has several grammar features that benefit from a compositional parser rather than a single generated LR grammar: nested types, closures, rich patterns, `match`, C-like expressions, FDN-backed metadata, and recoverable syntax errors. Chumsky provides recursive parsers, precedence/Pratt machinery, custom token streams with spans, and explicit recovery strategies while returning ordinary Rust AST values.

For a bootstrap compiler this gives a better iteration/debugging trade-off than immediately maintaining a large generated parse table.

Potential costs are Rust compile time and large combinator types. Mitigations:

- box/simplify parsers at deliberate grammar boundaries;
- keep lexing separate;
- pin the dependency versions during bootstrap;
- preserve an implementation-independent AST boundary so the parser library can be replaced later.

## Why Logos

Forge's lexical grammar is conventional enough for a generated deterministic lexer. Logos keeps token definitions adjacent to the Rust `Token` enum and supplies byte spans. It also cleanly feeds a Chumsky token stream.

## Alternatives considered

### Hand-written recursive descent + Pratt

This remains a credible long-term option and would give maximum control and minimum third-party dependency risk. It was the original Forge bootstrap plan. The Rust Chumsky implementation is preferred now because it reduces initial parser code, makes recursive grammar evolution faster, and gives recovery infrastructure immediately. The AST boundary means a future hand-written parser can replace it without affecting HIR/FIR.

### LALRPOP

A generated LR parser is attractive for a stable grammar and can provide strong deterministic parsing. Forge's early grammar is still moving, however, and advanced pattern syntax plus FDN integration make rapid compositional changes more valuable during bring-up.

### Pest

PEG grammars are concise and easy to read. The parser-to-typed-AST/recovery layer would still require substantial custom work, and PEG ordered choice can make error behavior sensitive to grammar ordering.

### Winnow

Winnow is an excellent low-level parser-combinator toolkit, especially for binary/text formats. Forge benefits more from Chumsky's compiler-language focus, recursive grammar facilities, rich errors, and recovery.

### Tree-sitter

Tree-sitter is highly attractive for editor/incremental syntax trees and may be added later for IDE tooling. Its concrete syntax tree should not become the compiler's semantic IR. The bootstrap compiler needs a compact typed Rust AST and compiler-directed recovery first.

## Expression parsing

The initial parser uses Chumsky's standard folded precedence combinators because they are straightforward for the current C-like operator table. Chumsky's Pratt facility is available if Forge later needs a more dynamic or denser operator definition. Operator precedence is a Forge language semantic and must remain covered by golden parser tests either way.

## Stability rule

No serialized shape emitted by Chumsky or Logos is part of the Forge language compatibility promise. Only Forge source semantics are long-term stable. `forge-parse --json` is explicitly a debug/tooling format until separately versioned.
