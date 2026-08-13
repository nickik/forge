# FDN v1 — Forge Data Notation

**Status:** normative for Forge v1 structured configuration and metadata.

FDN is an EDN-inspired immutable data notation used for package manifests, compiler/tool metadata, documentation, tests, user configuration, and payloads of Forge reader tags.

FDN is deliberately not a programming language.

## 1. Values

FDN supports:

- `nil`;
- booleans `true`, `false`;
- arbitrary-precision parsed integers;
- floating-point literals;
- UTF-8 strings;
- characters;
- keywords;
- symbols;
- vectors;
- lists;
- maps;
- sets;
- tagged reader values.

`nil` exists in FDN even though Forge source has no `null`. Decoding FDN `nil` into `T?` yields `None`.

## 2. Numbers

```fdn
42
-17
0xff
0b1010
0o755
3.14159
-1.0e6
```

The reader preserves enough information for schema/type decoding to diagnose overflow. Integer width is not inferred from host machine width.

## 3. Strings and characters

```fdn
"hello"
"Zürich\n"
\a
\space
\newline
\tab
```

Strings are UTF-8 values.

## 4. Keywords

Keywords begin with `:`:

```fdn
:name
:version
:forge/package
```

Keywords are values and are preferred as configuration-map keys.

## 5. Symbols

```fdn
Lighting
std/io
forge/compiler
```

Symbols are identifiers/data references rather than strings.

## 6. Vectors

```fdn
[1 2 3]
[:vax :lighting :pdp11]
```

Commas are accepted as whitespace but canonical FDN omits them.

## 7. Lists

```fdn
(and :threads :shared-memory)
```

Lists are data. FDN does not evaluate them.

## 8. Maps

```fdn
{
    :name "dec.graphics"
    :version #version "1.4.0"
}
```

Duplicate keys are invalid.

Any FDN value can technically be a key; keywords/symbols/strings are recommended.

## 9. Sets

```fdn
#{:read :write :execute}
```

Duplicate set values are invalid.

## 10. Comments and discard

Line comments:

```fdn
; EDN-style comment
// C/Forge-style comment
```

Block comment:

```fdn
/* block comment */
```

Discard exactly one following complete value:

```fdn
#_ {:old true}
```

## 11. Reader tags

Syntax:

```text
#tag value
```

Examples:

```fdn
#uuid "550e8400-e29b-41d4-a716-446655440000"
#inst "1985-04-12T23:20:50.52Z"
#net/ip "192.0.2.1"
```

A reader tag is not textual substitution. It receives the already parsed following FDN value and returns a typed reader value or an error.

Libraries should use namespaced tags. Unqualified tags are reserved for standardized forms.

## 12. Built-in v1 reader tags

### `#uuid`

```fdn
#uuid "550e8400-e29b-41d4-a716-446655440000"
```

Represents a 128-bit UUID.

### `#inst`

```fdn
#inst "1985-04-12T23:20:50.52Z"
```

Represents an absolute timestamp.

### `#duration`

```fdn
#duration "250ms"
#duration "5s"
#duration "2h"
```

Represents an exact normalized duration.

### `#size`

```fdn
#size "64KiB"
#size "4MiB"
```

Represents an exact byte count.

### `#bytes`

```fdn
#bytes "DE AD BE EF"
#bytes [0xde 0xad 0xbe 0xef]
```

Represents immutable bytes.

### `#path`

```fdn
#path "src/parser.forge"
```

Represents a platform-neutral path value; normalization is a tool decision, not string rewriting by the reader.

### `#version`

```fdn
#version "1.2.3"
```

Represents a structured package/tool version.

### `#version-range`

```fdn
#version-range ">=1.2 <2.0"
```

Represents a package-version constraint.

### `#type`

```fdn
#type "u32"
#type "std.net.Address"
```

Represents a Forge type description for tools/code generation. Parsing it does not instantiate or execute Forge code.

### `#ref`

```fdn
#ref "std.mem.Arena"
```

Represents a reference to a Forge declaration/symbol.

### `#uri`

```fdn
#uri "https://example.org/spec"
```

Represents a validated URI value.

### `#forge/code`

```fdn
#forge/code "val x: u32 = 1;"
```

Represents Forge source embedded in documentation/tooling data.

## 13. Custom readers

Forge programs/tools may register a reader by qualified tag:

```forge
fdn.register_reader(
    :gfx/color,
    read_color
)?;
```

Then:

```fdn
#gfx/color "#ff8000"
```

Reader hooks receive a parsed FDN value. They do not gain arbitrary lexer control and cannot redefine core syntax.

## 14. Unknown tags

Readers support:

- **strict mode:** unknown tag is an error;
- **preserving mode:** produce `TaggedValue{tag, value}` unchanged.

Editors, formatters and package indexes should normally use preserving mode.

## 15. Metadata integration

Forge `@{ ... }` embeds an FDN map as metadata:

```forge
@{
    :doc/category :network
    :since #version "1.0.0"
    :doc/see [#ref "std.net.Address"]
}
pub fn connect(...) { ... }
```

Simple compiler-known attributes such as `@inline` are shorthand for structured metadata with compiler-defined semantics.

## 16. Package manifest

Recommended `package.fdn`:

```fdn
{
    :package {
        :name dec.graphics
        :version #version "1.0.0"
        :id #uuid "550e8400-e29b-41d4-a716-446655440000"
    }

    :sources [#path "src"]
    :targets #{:vax :lighting}

    :dependencies {
        dec.math #version-range ">=1.0 <2.0"
    }

    :build {
        :bounds-check true
        :overflow :checked
        :optimization :speed
    }
}
```

## 17. Documentation

Documentation can be stored/attached as FDN and rendered into terminals, printed manuals, IDE help or other formats.

```fdn
{
    :title "Arena Allocators"
    :summary "Memory grouped by lifetime."
    :see [#ref "std.mem.Pool" #ref "std.mem.Slab"]
    :example #forge/code "val mark = arena.mark(); defer arena.release(mark);"
}
```

## 18. Canonical FDN

Canonical serialization is required when hashing/signing/reproducible-build inputs need stable bytes. Canonical mode defines normalized escaping, number spelling and deterministic map/set ordering. Human-written FDN is not required to be canonically ordered.

## 19. Non-features

FDN has no variables, assignment, functions, loops, conditionals, implicit includes, environment expansion or arbitrary execution. Computation belongs in Forge or a purpose-built tool.
