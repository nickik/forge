# Rust parser status

## Implemented in the initial parser

- `module` declaration
- `import` declarations
- `pub`
- `fn` and `nfn`
- typed parameters and optional defaults
- return types
- `struct`
- `enum`
- `tagged`
- `distinct`
- type aliases
- global and local `val` / `var` / `const`
- named, pointer, reference, optional, slice, fixed-array, `Result`, function and closure types
- integer, float, string and boolean literals
- paths such as `std.io.println`
- structure initializers
- arrays
- function calls
- indexing
- unary operators
- Forge arithmetic, comparison, bitwise and logical operator precedence
- blocks
- expression statements
- `return` / `return tail`
- `if` / `else`
- `while`
- `defer`
- `unsafe`
- source byte spans on AST nodes
- JSON AST dumping
- basic parser recovery at `;` / `}` boundaries

## Intentionally next

The parser is a bring-up frontend, not yet the full Forge v1 grammar. The next parser work should add, in roughly this order:

1. assignments and compound assignments;
2. complete type grammar and method-call/member distinction;
3. closures and capture lists;
4. patterns and destructuring;
5. `match` with guards and nested patterns;
6. `for` and `switch`;
7. `@` metadata carrying FDN values;
8. `#tag` reader forms and FDN payload integration;
9. named `nfn` calls;
10. CSP `select` syntax;
11. deliberate error AST nodes and stronger recovery;
12. lossless token/trivia representation for formatter/IDE tooling if needed.

## Non-goal

Do not add LLVM code generation to the parser crate. Add semantic lowering first.
