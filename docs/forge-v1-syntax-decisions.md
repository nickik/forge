# Forge v1 syntax decisions

**Status:** normative clarification to `forge-v1-spec.md` until the main specification is mechanically reconciled.

These decisions resolve ambiguities discovered during the parser/conformance audit. If this file conflicts with an older example in `forge-v1-spec.md`, this file wins for Forge v1.

## Visibility

- `pub` is the only explicit visibility modifier in v1.
- Module-private is the default.
- `internal` is reserved for future use and is not legal Forge v1 syntax.

## Declarations and assignment

Every `val`, `var`, and `const` declaration requires an initializer.

```forge
val x: i32 = 1;
var y: i32 = 2;
const PAGE_SIZE: usize = 4096;
```

The following are invalid:

```forge
val x: i32;
var y: i32;
const z: i32;
```

Assignment is a **statement**, not an expression:

```forge
x = x + 1;
```

Assignment may not appear where a value expression is required.

Forge v1 has **no compound-assignment operators**. The following are invalid:

```forge
x += 1;
x -= 1;
x *= 2;
x /= 2;
x %= 2;
x &= mask;
x |= mask;
x ^= mask;
x <<= 1;
x >>= 1;
```

Use an ordinary assignment instead.

## Control-flow syntax

Parentheses are mandatory around conditions/headers for `if`, `while`, and `for`.

```forge
if (ready) { ... }
while (running) { ... }
for (var i: usize = 0; i < count; i = i + 1) { ... }
```

A C-style `for` header has:

- initializer: declaration, assignment, expression statement, or empty;
- condition: expression or empty;
- step: assignment, expression, or empty.

`switch` is **not part of Forge v1**. Use `match`.

## Patterns

Forge v1 patterns include:

- `_` wildcard;
- bindings;
- literals;
- inclusive/exclusive ranges;
- qualified enum/tagged-union variants;
- struct/variant destructuring;
- sequence destructuring and `..rest`;
- map/keyword destructuring;
- OR patterns with `|`;
- as-binding `name @ pattern`;
- guards with `when`.

Forge v1 does **not** have pattern conjunction `p1 & p2` or pattern negation `!pattern`.

Map pattern field semantics:

```forge
{
    :name name,
    :email email?,
    ..
}
```

- `name` means the key must exist;
- `email?` means the key may be absent and the binding is optional;
- `..` ignores remaining entries.

## Variants and Option

Enum variants must be qualified:

```forge
Color::Red
```

Bare `Red` does not denote an enum variant merely because contextual type information is available.

Tagged-union construction uses the qualified constructor form:

```forge
Shape::Circle{radius: 4.0}
```

For unit variants, both forms are legal:

```forge
Shape::Point
Shape::Point{}
```

The formatter should prefer the bare form.

Built-in Option constructors do not require an `Option::` prefix:

```forge
None
None{}
Some(value)
```

`None` and `None{}` are equivalent. The formatter should prefer `None`.

## Closures

A closure with captures uses an explicit capture list:

```forge
[factor](x: u32) -> u32 {
    return x * factor;
}
```

Mutable-reference capture:

```forge
[&mut count]() -> u32 {
    count = count + 1;
    return count;
}
```

When there are no captures, `[]` is omitted:

```forge
(x: u32) -> u32 {
    return x + 1;
}
```

`[](x: u32) -> u32 { ... }` is not the canonical v1 syntax and should be rejected.

## Metadata

Metadata is legal before declarations and as postfix metadata on types and expressions.

```forge
@repr(c)
struct Header { ... }

type Percentage = u8 @range(0..=100);

unsafe {
    val x = values[i] @unchecked;
}
```

The parser accepts the general metadata position; semantic validation decides which metadata names are legal for which construct.

Checked arithmetic is the default. `@check(...)` is removed from v1 and should be rejected as a standardized compiler form. Explicit wrapping remains available through `@wrap(...)` / `@overflow(wrap)` as defined by the metadata contract.

## Methods

`impl` provides statically associated methods. Method-call syntax is supported:

```forge
p.length()
```

and resolves statically to an associated method such as `Point::length(&p)` according to the type checker. Ordinary structs do not contain method pointers or vtables.

## FFI and volatile memory

`extern` and `volatile` are Forge v1 keywords.

```forge
extern "C" {
    fn qsort(...);
}

val register: *volatile u32 = ...;
```

`volatile` is a type qualifier for raw memory access; actual dereference/access remains subject to `unsafe` rules.

## Reserved syntax

`internal` and all compound-assignment spellings are reserved/rejected in v1 so future versions can define them without changing the meaning of accepted v1 programs.
