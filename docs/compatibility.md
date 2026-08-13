# Forge v1 Compatibility Contract

Forge v1 is intended to remain source-compatible for decades.

## Frozen source semantics

Once v1 is declared stable, a conforming v1 program must not change meaning merely because a newer compiler is used.

The following are especially frozen:

- lexical meaning of existing tokens;
- operator precedence and evaluation order;
- `val`/`var` mutation rules;
- type-after-name declarations;
- `T?` as `Option[T]`, with no language-level `null`;
- strict conversion rules;
- default bounds and overflow behavior;
- pattern-match ordering and exhaustiveness rules;
- `Result[T,E]` propagation semantics;
- `defer` scope-exit order;
- `unsafe` boundary semantics;
- public representation guarantees requested by `@repr`;
- FDN core data grammar and built-in tag meanings.

## Extensions

New syntax may be added only when old token streams remain unambiguous. Existing keywords, reader tags, metadata keys and operators are never repurposed.

User-defined `#reader` namespaces are reserved to their owners. DEC/Forge built-ins use reserved namespaces.

## ABI

Forge does not promise a single universal ABI for all ordinary Forge types. Stable ABI is explicitly requested using representation/calling-convention metadata, especially `@repr(c)` and `extern "C"`.

## Standard library

Language stability and library stability are separate. Long-lived foundational APIs are versioned conservatively. Experimental APIs must not be promoted silently into frozen core APIs.

## Diagnostics

Diagnostic wording is not source ABI, but diagnostic categories and machine-readable error identifiers should remain stable once published.
