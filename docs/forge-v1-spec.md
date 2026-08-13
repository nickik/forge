# Forge v1 Language Specification

**Status:** v1 design baseline for bootstrap implementation.  
**Audience:** compiler implementers, systems programmers, tool authors.  
**Compatibility intent:** once Forge v1 is declared stable, conforming v1 source is intended to remain valid for decades.

Forge is a statically typed, ahead-of-time compiled systems language. It keeps the operational transparency and familiar surface of C while replacing major sources of accidental complexity: implicit conversions, unchecked arrays by default, untagged unions as ordinary data modeling, textual preprocessing, sentinel/null error conventions, uninitialized reads, and invisible ownership conventions.

Forge v1 does **not** expose user-defined parametric generics. `Option`/`Result`, arrays, slices and callable signatures are compiler-defined type constructors and do not imply a general generic system.

---

## 1. Design invariants

A conforming Forge implementation must preserve these principles:

1. Ordinary data has direct value layout unless an explicit abstraction requests otherwise.
2. Allocation is never an unavoidable hidden consequence of ordinary language operations.
3. `val` is immutable by default; mutation is explicit.
4. There is no language-level `null`.
5. Absence is represented by `T?`, semantically `Option[T]`.
6. Recoverable errors are values, normally `Result[T,E]`.
7. Raw memory escape hatches are visibly inside `unsafe`.
8. Arrays/slices are bounds checked by default, including production semantics.
9. Ordinary integer arithmetic is checked by default; wrapping is explicit.
10. Numeric and pointer conversions are strict and explicit.
11. Evaluation order is defined.
12. Modules are semantic units; there is no textual include preprocessor.
13. `#tag` is structured reader extension; `@` is metadata.
14. Durable allocation is explicit through an allocator or owning object; temporary non-escaping work may use the narrow execution context.
15. The language does not require garbage collection, exceptions, green threads, inheritance or virtual dispatch.

---

# Part I — Lexical and source structure

## 2. Source text

Forge source is UTF-8. Implementations must preserve byte-accurate source spans for diagnostics.

Line comments:

```forge
// comment
```

Nested block comments are allowed:

```forge
/* outer
   /* inner */
*/
```

Semicolons terminate simple statements and field declarations. Braces delimit blocks.

## 3. Identifiers and keywords

Identifiers use ASCII letters/underscore followed by letters/digits/underscore in v1 portable source. Tooling may permit additional Unicode identifiers only behind an explicit extension mode.

Core keywords include:

```text
module import pub internal
val var const
fn nfn return tail
struct enum tagged bitstruct distinct type impl
if else while for in break continue
match when switch case default
unsafe defer with context
true false None
select recv timeout
```

`None` is an `Option` constructor, not `null`.

## 4. Literals

Integers:

```forge
0
42
-17
0xff
0b1010
0o755
42u32
7i16
```

Floating point:

```forge
1.0
3.14f32
1.0e6f64
```

Characters:

```forge
'a'
'\n'
```

Strings:

```forge
"hello"
"UTF-8: Zürich"
c"NUL-terminated C string"
```

Ordinary string literals produce immutable `str`. `c"..."` is an explicit C-compatible NUL-terminated literal for FFI.

Untyped numeric literals are compile-time mathematical values until context assigns a concrete numeric type. If context does not determine exactly one valid type, the programmer must suffix/cast explicitly.

---

# Part II — Modules, declarations and values

## 5. Modules

Every source file belongs to a module:

```forge
module gfx.raster;
```

Imports are semantic:

```forge
import std.io;
import std.mem, std.str;
```

No source text is pasted by import.

Visibility:

```forge
pub fn draw(...) { ... }
fn helper(...) { ... }
```

Module-private is the default.

## 6. Type-after-name declarations

Forge declarations use `name: Type`:

```forge
val count: u32 = 10;
var position: Point = Point{x: 0.0, y: 0.0};
```

Function parameters follow the same rule:

```forge
fn add(a: i32, b: i32) -> i32 {
    return a + b;
}
```

## 7. `val`, `var`, `const`

`val` creates an immutable binding:

```forge
val x: i32 = 10;
```

`var` creates a mutable binding:

```forge
var x: i32 = 10;
x = 11;
```

`const` defines a compile-time constant:

```forge
const PAGE_SIZE: usize = 4096;
```

Local type inference:

```forge
val x = 10u32;
val name = "Ada";
```

Inference never authorizes otherwise-illegal implicit conversion.

## 8. Definite initialization

Every read must be dominated by an initialization on every reachable path.

Invalid:

```forge
var x: i32;
print(x); // error: possibly uninitialized
```

Valid:

```forge
var x: i32;
if (condition) {
    x = 1;
} else {
    x = 2;
}
print(x);
```

Optional variables are not automatically initialized:

```forge
var p: &File?;
use(p); // error until assigned
```

Explicit absence:

```forge
var p: &File? = None;
```

---

# Part III — Types

## 9. Fundamental types

Fixed-width integers:

```text
i8 i16 i32 i64
u8 u16 u32 u64
```

Pointer-width integers:

```text
isize usize
```

Floating point:

```text
f32 f64
```

Other fundamental types:

```text
bool char byte void never
```

Forge has no implementation-dependent source-level `int` type.

## 10. Structs

Structs are value types by default:

```forge
struct Point {
    x: f32;
    y: f32;
}
```

Construction:

```forge
val p: Point = Point{x: 1.0, y: 2.0};
```

An array of `Point` contains points inline; there is no implicit heap indirection or object header.

Field order/layout follows the target's ordinary Forge layout unless representation metadata specifies otherwise.

## 11. Enums

```forge
enum Color {
    Red,
    Green,
    Blue,
}
```

Enums are strongly typed and do not implicitly convert to integers.

Representation can be pinned:

```forge
@repr(u8)
enum Color { Red, Green, Blue }
```

Even with `@repr(u8)`, integer conversion remains explicit.

## 12. Tagged unions

```forge
tagged Shape {
    Circle { radius: f32; },
    Rectangle { width: f32; height: f32; },
    Point,
}
```

Construction:

```forge
val s: Shape = Shape::Circle{radius: 4.0};
```

The active variant is always known in safe code.

Optional representation:

```forge
@repr(tag: u8)
tagged Token {
    Identifier { text: str; },
    Number { value: i64; },
    Plus,
}
```

## 13. Distinct types

```forge
distinct UserId: u32;
distinct AccountId: u32;
```

`UserId` and `AccountId` have the same representation but are not mutually assignable or comparable without explicit conversion.

This is a zero-runtime-cost nominal type distinction.

## 14. Type aliases

```forge
type ByteCount = usize;
```

Unlike `distinct`, aliases do not create a new nominal type.

## 15. Range-constrained types

```forge
type Percentage = u8 @range(0..=100);
```

Construction from an arbitrary integer checks range:

```forge
val p: Percentage = Percentage(value); // traps on invalid value
```

Fallible conversion:

```forge
val p: Percentage? = Percentage.try(value);
```

## 16. Fixed arrays

```forge
val pixels: [Pixel; 1024] = ...;
```

`[T; N]` is a value containing `N` inline `T` values.

Array literals:

```forge
val a: [u32; 4] = [1, 2, 3, 4];
```

## 17. Slices

A slice is a non-owning `(pointer,length)` view.

```forge
fn sum(values: u32[]) -> u64 { ... }
```

Writable slice:

```forge
fn clear(values: u32[] mut) {
    for (var i: usize = 0; i < values.len; i += 1) {
        values[i] = 0;
    }
}
```

Read-only is the default.

## 18. Safe references

Read reference:

```forge
&Point
```

Mutable reference:

```forge
&mut Point
```

Mutation through a reference requires `&mut`:

```forge
fn translate(p: &mut Point, dx: f32, dy: f32) {
    p.x += dx;
    p.y += dy;
}
```

Forge v1 does not implement a Rust-style whole-program borrow checker. The compiler enforces straightforward local exclusivity rules and rejects obviously overlapping mutable/reference use. More sophisticated alias guarantees may be expressed by library/API metadata such as `@unique` but must not silently change v1 reference semantics.

## 19. Raw pointers

```forge
*Point
*void
```

Raw pointers are non-null values at the language level. Their representation may include machine address zero only in unsafe/FFI construction or as an internal representation of an optional pointer.

Dereference, pointer arithmetic and arbitrary pointer casts require `unsafe`.

## 20. No `null`; optional values

Forge has no `null` literal and no nullable ordinary type.

`T?` means `Option[T]`:

```forge
fn find(id: UserId) -> &User?;
```

Conceptually:

```text
Option[T] = Some(T) | None
```

Constructors/patterns:

```forge
val p: &User? = Some(&user);
val q: &User? = None;
```

A lossless promotion from `T` to `T?` is permitted:

```forge
val p: &User? = &user; // equivalent to Some(&user)
```

The compiler may use niche representations. For example `*T?` can occupy one pointer word with zero bits representing `None`. This is an implementation/layout optimization; source semantics still contain no null value.

Arrays/collections of optional/tagged values may use packed/sidecar discriminants when their concrete generated type requests such a layout.

## 21. `Result`

`Result[T,E]` is a compiler-defined sum type:

```text
Ok(T) | Err(E)
```

Example:

```forge
fn open(path: str) -> Result[File, IOError] {
    ...
}
```

`Result` is available despite the absence of general user-defined generics.

---

# Part IV — Conversions, arithmetic and evaluation

## 22. Strict conversions

Forge has no C integer promotion rules.

Invalid:

```forge
val a: u8 = 1;
val b: u32 = 2;
val c = a + b;
```

Valid:

```forge
val c: u32 = u32(a) + b;
```

Rules:

- no implicit signed/unsigned mixing;
- no implicit integer/floating conversion;
- no `bool`/integer conversion;
- no enum/integer conversion;
- no array-to-pointer decay;
- no pointer/integer conversion in safe code;
- no implicit narrowing/widening between concrete numeric types.

Contextual typing of a literal is not an implicit runtime conversion:

```forge
val x: u32 = 1;
```

## 23. Logical and bitwise operators

Logical bool operators:

```forge
!a
a && b
a || b
a xor b
```

`&&` and `||` short circuit. `xor` evaluates both operands and yields boolean exclusive-or.

Bitwise integer operators:

```forge
~x
x & y
x | y
x ^ y
x << n
x >> n
```

The C punctuation is retained, but logical and bitwise domains do not coerce into each other.

## 24. Overflow

Ordinary integer arithmetic is checked:

```forge
val c: u32 = a + b;
```

If the mathematical result is not representable, execution performs a defined arithmetic trap/panic path.

Explicit wrapping:

```forge
val c: u32 = @wrap(a + b);
```

Function/block metadata may declare wrapping arithmetic where appropriate:

```forge
@overflow(wrap)
fn hash_mix(x: u32) -> u32 {
    return x * 2654435761u32;
}
```

Unchecked overflow is never implied solely by optimization level.

## 25. Division and shifts

Division by zero is a defined trap.

Shift counts outside the defined range are a defined trap in checked operations. Explicit wrapping/masked shift intrinsics may be provided for algorithms that require them.

Signed right shift semantics must be specified per operator/intrinsic and never left to host-C implementation accident.

## 26. Evaluation order

Forge evaluates expression operands and function arguments left-to-right except where short-circuit/operator semantics explicitly say an operand is skipped.

This is source semantics and cannot be changed by optimization.

---

# Part V — Control flow and patterns

## 27. `if`, loops and iteration

```forge
if (ready) {
    run();
} else {
    stop();
}
```

```forge
while (condition) {
    ...
}
```

C-style loop retained:

```forge
for (var i: usize = 0; i < count; i += 1) {
    ...
}
```

Value iteration:

```forge
for (val item in items) {
    process(item);
}
```

`break` and `continue` have ordinary lexical-loop meaning.

## 28. `switch`

A C-shaped `switch` exists for simple integer/enum porting:

```forge
switch (opcode) {
case 1:
    one();
    break;
case 2:
    two();
    break;
default:
    unknown();
}
```

Fallthrough is never implicit. It requires explicit `@fallthrough;`.

New data-oriented code should generally use `match`.

## 29. `match`

```forge
match (shape) {
    Shape::Circle{radius} => {
        return PI * radius * radius;
    },
    Shape::Rectangle{width, height} => {
        return width * height;
    },
    Shape::Point => {
        return 0.0;
    },
}
```

Closed enum/tagged-union matches must be exhaustive unless a wildcard covers remaining possibilities.

Arms are tested according to pattern-decision semantics preserving source priority where patterns overlap.

## 30. Pattern forms

### Wildcard

```forge
_
```

### Binding

```forge
value
```

### Literal/range

```forge
0
200..=299
'a'
```

### Enum/variant

```forge
Color::Red
Result::Ok{value}
```

### Struct/variant destructuring

```forge
Point{x, y}
Rect{width: w, height: h}
```

### Sequence

```forge
[first, second, ..rest]
```

### Map/keyword destructuring

```forge
{
    :name name,
    :age age,
    :email email?,
    ..
}
```

Map patterns perform typed key lookups according to the matched collection's pattern protocol. This is not a general dynamic-map requirement on all structs.

### Or

```forge
Token::Plus | Token::Minus
```

Bindings in alternatives must bind compatible names/types.

### And

```forge
binding @ pattern
```

The recommended as-binding form is:

```forge
whole @ Shape::Circle{radius}
```

Forge may additionally support logical pattern conjunction `p1 & p2` when both can be checked without ambiguity. The exact binding compatibility rules are normative: a binding may not receive different types depending on branch.

### Negation

`!pattern` is allowed only when it introduces no bindings and the pattern domain supports decidable complement.

### Guards

```forge
value when value > 0 => ...
```

Guards run only after the structural pattern succeeds.

## 31. Destructuring declarations

Patterns can appear in `val`/`var` bindings only when they are statically irrefutable.

Valid:

```forge
val Point{x, y} = point;
```

A length-dependent slice pattern is refutable and must use `match` or an explicit conditional-match form:

```forge
match (values) {
    [a, b, ..rest] => use(a, b, rest),
    _ => handle_short(),
}
```

This prevents hidden match-failure traps in ordinary bindings.

## 32. Bitstructs

```forge
bitstruct Status: u16 {
    ready: 1;
    error: 1;
    mode: 3;
    code: 5;
    reserved: 6;
}
```

Bit-field layout is specified by Forge metadata/target bit order, not host C bitfield behavior. Access lowers to masks/shifts.

---

# Part VI — Functions, calls, closures and errors

## 33. Positional functions: `fn`

```forge
fn clamp(value: i32, low: i32, high: i32) -> i32 {
    ...
}
```

Call:

```forge
clamp(x, 0, 100);
```

Forge v1 `fn` parameters have no default arguments. Calls are positional only.

## 34. Named functions: `nfn`

`nfn` is named-only and may declare defaults:

```forge
nfn connect(
    host: str,
    port: u16 = 443,
    encrypted: bool = true
) -> Result[Connection, NetError] {
    ...
}
```

Call:

```forge
connect(:host = "server.example");
```

or:

```forge
connect(
    :encrypted = false,
    :port = 8080,
    :host = "localhost"
);
```

Rules:

- all call arguments are named;
- positional and named forms never mix;
- names must be literal parameter names;
- order does not matter;
- duplicate/unknown names are errors;
- omitted parameters must have defaults;
- default expansion is compile-time/static;
- parameter names are part of public source API for public `nfn`.

## 35. Exact static overloads

Forge v1 permits multiple declarations with the same name when argument types distinguish them exactly:

```forge
fn dot(a: Vec2f, b: Vec2f) -> f32 { ... }
fn dot(a: Vec2i, b: Vec2i) -> i32 { ... }
```

Resolution uses no implicit numeric conversion. Return type alone never selects an overload. Ambiguity is a compile error.

## 36. Function pointer type

```forge
fn(u32) -> u32
```

Example:

```forge
fn square(x: u32) -> u32 { return x * x; }
val op: fn(u32) -> u32 = square;
```

A function pointer has no capture environment.

## 37. Closures

Forge v1 supports non-escaping closures with explicit capture lists using C++-familiar syntax.

Capture by value:

```forge
val factor: u32 = 4;
val scale = [factor](x: u32) -> u32 {
    return x * factor;
};
```

Capture mutable reference explicitly:

```forge
var count: u32 = 0;
val next = [&mut count]() -> u32 {
    count += 1;
    return count;
};
```

Capture-free:

```forge
val inc = [](x: u32) -> u32 { return x + 1; };
```

Capture-free closures may coerce to matching function-pointer type.

Conceptual closure representation:

```text
{ code_pointer, environment_pointer }
```

The compiler normally stores the environment in lexical storage. v1 does not silently heap-box an escaping environment.

A closure may be passed to a parameter typed:

```forge
closure(u32) -> u32
```

but cannot be returned/stored past capture lifetime unless a library explicitly constructs an owned callable representation.

## 38. Higher-order functions

Concrete higher-order functions are permitted without language generics:

```forge
fn apply_twice(value: u32, f: closure(u32) -> u32) -> u32 {
    return f(f(value));
}
```

Concrete standard-library families and compile-time type generation supply common typed algorithms.

Transducers are a standard library/code-generation facility for fusing maps/filters/reductions over concrete element types; they are not special dynamic runtime iteration objects.

## 39. `Result` propagation

```forge
fn load(path: str) -> Result[Config, IOError] {
    val file = open(path)?;
    val data = read_all(file)?;
    return parse(data);
}
```

Postfix `?` on `Result[T,E]` unwraps `Ok(T)` or returns the compatible `Err(E)` from the current function.

`?` on optional values is not used for the same propagation syntax in v1; optional handling uses pattern matching or dedicated optional combinators, avoiding ambiguity between "not present" and "error".

## 40. Panic

```forge
panic("internal invariant violated");
```

Panic represents unrecoverable failure. Baseline semantics terminate the current process/program execution unit; v1 does not require stack unwinding.

## 41. No exceptions

Forge v1 has no language exceptions. Libraries may implement condition/restart frameworks using explicit control structures, but ordinary Forge calls cannot invisibly throw through arbitrary stack frames.

## 42. `defer`

```forge
val file = file_open(path)?;
defer file_close(file);
```

Deferred actions run at exit from the current lexical scope, including via `return`, `break`, `continue`, and `?` propagation.

Multiple defers run LIFO:

```forge
lock(a);
defer unlock(a);
lock(b);
defer unlock(b);
```

Block form:

```forge
defer {
    release(buffer);
    trace("released");
}
```

`defer` is lexically lowered and does not require heap closure allocation.

## 43. Tail calls

Ordinary tail calls may be optimized.

Required tail call:

```forge
return tail loop(next, state);
```

If the backend cannot compile the marked call without growing the logical call stack, compilation fails. This gives programmers a reliable recursion tool without requiring universal proper tail calls.

---

# Part VII — Safety and raw operations

## 44. Bounds checking

Array and slice indexing is checked in ordinary code:

```forge
val x = values[i];
```

Out of bounds causes a defined trap/panic.

The compiler should prove and remove redundant checks where possible.

Unchecked indexing requires explicit unsafe intent:

```forge
unsafe {
    val x = values[i] @unchecked;
}
```

Optimization level alone never changes checked source semantics.

## 45. `unsafe`

```forge
unsafe {
    val p: *u32 = ptr_from_address[*u32](address);
    val x: u32 = *p;
}
```

Operations requiring unsafe include at least:

- raw pointer dereference;
- raw pointer arithmetic;
- pointer/integer conversion;
- arbitrary pointer reinterpretation/type punning;
- unchecked indexing;
- inactive raw-union field access if raw unions are exposed by FFI;
- volatile MMIO;
- inline assembly;
- calls to APIs explicitly marked unsafe.

`unsafe` does not disable static typing, definite initialization, or unrelated checks.

## 46. Undefined-behavior policy

Forge specifications should classify low-level behavior into:

1. defined result;
2. defined trap;
3. explicitly target-defined behavior;
4. genuinely undefined behavior possible only through unsafe contract violation.

Category 4 must remain small. Safe source should not rely on host-language undefined behavior.

---

# Part VIII — Memory, context and ownership conventions

## 47. Allocation is explicit

Forge language syntax does not imply a universal heap allocator.

Any operation that creates durable memory must identify ownership through:

- an explicit `&Allocator` argument; or
- a receiver/owning object that already contains/owns its memory domain.

Example explicit allocation:

```forge
fn decode_image(bytes: u8[], allocator: &Allocator)
    -> Result[Image, DecodeError];
```

Example owner-based allocation:

```forge
fn spawn(world: &mut World, spec: EntitySpec)
    -> Result[Entity, AllocError];
```

## 48. Primitive allocation is fallible

Standard allocator operations return `Result`. Allocation failure is not automatically panic.

Applications may explicitly turn failure into panic when appropriate.

Fallible allocation results are `@must_use` in the standard library.

## 49. Scratch context

Forge has a narrow implicit execution context for ambient non-owning services:

```forge
context.scratch
context.logger
context.clock
context.random
context.trace
```

`context.scratch` may be used for memory whose lifetime cannot escape the active scratch region/call tree.

Scoped override:

```forge
with context {
    :scratch &test_scratch
    :logger &test_logger
} {
    run_test();
}
```

Applications cannot extend core context into a general hidden service locator. Durable business/system dependencies remain explicit.

## 50. Arenas and pools

Arena/pool behavior is primarily standard-library policy rather than unique language syntax. Forge code can create generated concrete types representing linear, stack, fixed-pool, slab, free-list, static and thread-local memory domains.

Bulk lifetime operations pair naturally with `defer`:

```forge
val mark = arena.mark();
defer arena.release(mark);
```

---

# Part IX — Structured compile-time data and metadata

## 51. FDN

Forge Data Notation (FDN) is specified separately in `fdn-v1-spec.md`. It is used for:

- package manifests;
- compiler/tool metadata;
- documentation;
- tests;
- reader-tag payloads;
- user configuration.

## 52. Keywords

Forge source recognizes keyword literals such as:

```forge
:name
:graphics
:forge/version
```

Keywords are immutable symbolic values suitable for maps/tags/metadata. They are distinct from text strings.

## 53. Reader forms `#tag`

Forge recognizes:

```forge
#uuid "550e8400-e29b-41d4-a716-446655440000"
#inst "1985-04-12T23:20:50.52Z"
```

General syntax is a reader tag followed by one FDN value. Reader hooks operate on structured parsed FDN, never arbitrary source-text substitution.

Built-in source reader hooks include the FDN built-ins plus Forge-specific compile-time generators in reserved namespaces.

Example generated array:

```forge
val table: [u32; 256] = #forge/array {
    :type #type "u32"
    :size 256
    :init 0
};
```

Example generated concrete vector type:

```forge
#forge/type {
    :template std/vector
    :element #type "u32"
    :name Vec_u32
}
```

The result of type generation is an ordinary concrete Forge type. It does not introduce runtime generics.

Custom reader hooks must be namespaced except for standardized built-ins.

## 54. Metadata `@`

Simple metadata:

```forge
@inline
fn hot(...) { ... }
```

Structured metadata embeds FDN:

```forge
@{
    :since #version "1.0.0"
    :doc/category :network
}
pub fn connect(...) { ... }
```

Metadata can be consumed by compiler, linker, documentation, serialization and static-analysis tools. Unknown metadata must not silently change core language semantics.

## 55. Representation metadata

V1 standardized metadata includes at least:

```text
@repr(c)
@repr(packed)
@repr(u8)
@repr(tag: u8)
@align(N)
@range(...)
@overflow(wrap|checked)
@inline
@cold
@must_use
@deprecated(...)
```

Target/FFI representation metadata is part of the compatibility contract once published.

---

# Part X — Object/data organization

## 56. Methods are static organization

Forge can associate functions with a type:

```forge
impl Point {
    fn length(self: &Point) -> f32 {
        return sqrt(self.x * self.x + self.y * self.y);
    }
}
```

This is static name organization/sugar. Ordinary structs do not contain method pointers, vtables or object headers.

## 57. No inheritance

Forge v1 has no class inheritance. Reuse is through:

- composition;
- concrete data structures;
- functions/systems;
- tagged unions;
- exact overloads;
- generated concrete types;
- ECS-style component stores where appropriate.

## 58. ECS orientation

ECS is a standard-library/programming architecture rather than a mandatory representation for every value.

Use plain structs for values and helper wrappers. Use ECS-style stores when many instances share component-oriented processing and locality benefits.

Example:

```forge
fn movement(world: &mut World, dt: f32) {
    for (val e, var p: Position, val v: Velocity in world.query()) {
        p.x += v.x * dt;
        p.y += v.y * dt;
    }
}
```

A `ManagedVec_u32` is simply a struct plus statically known methods, not an ECS entity and not a function-pointer object.

---

# Part XI — Concurrency

## 59. Threads

Forge's standard thread API maps to operating-system threads on supported targets. No language-level green-thread scheduler is required.

Each thread receives an execution context and normally its own scratch arena.

## 60. CSP `select`

Typed channels are standard-library values. Multi-channel waiting is represented by `select` syntax:

```forge
select {
    recv jobs -> job => {
        process(job);
    }

    recv shutdown -> _ => {
        return;
    }

    timeout #duration "100ms" => {
        maintenance();
    }
}
```

The backend/runtime library maps this to available OS/thread/process facilities.

## 61. Agents

Data-owning agents are a standard-library abstraction: send an action/function plus arguments to a worker that owns mutable state. Within one process the action may be a function pointer plus copied argument record; across processes it must use stable action identifiers/data, not raw code addresses.

Agents are not a replacement for ordinary functions or structs.

---

# Part XII — Foreign interfaces and layout

## 62. C ABI

```forge
extern "C" {
    fn qsort(base: *void, n: usize, width: usize, cmp: fn(*void, *void) -> i32) -> void;
}
```

C nullable pointers are imported as optional raw pointers where applicable. Conversion between C's null representation and Forge `None` happens at the ABI boundary.

C varargs are allowed only through explicit foreign declarations, not as ordinary Forge variadic functions in v1.

## 63. `@repr(c)`

```forge
@repr(c)
struct CPoint {
    x: i32;
    y: i32;
}
```

Pins the representation according to the target's documented C ABI. Ordinary Forge representation may evolve by compiler version only within the compatibility rules for non-pinned private layouts.

---

# Part XIII — Standard library expectations relevant to language use

## 64. Strings

`str` is immutable borrowed text with pointer+byte length semantics. It is not NUL terminated.

Owned mutable strings are concrete standard-library types using explicit allocators, e.g. generated/common `String` implementations. APIs must distinguish text from byte arrays.

UTF-8 is the standard text encoding; byte indexing and Unicode-scalar/grapheme iteration are distinct operations.

## 65. Collections

Because v1 has no user generics, standard concrete families are generated or predefined:

```text
Vec_u8
Vec_u32
Vec_Point
HashMap_Symbol_User
Pool_Connection
```

The fundamental vector representation should not need to store an allocator. Managed wrappers may store one when convenient.

## 66. Transducers

The standard library provides compile-time-generated typed transducer pipelines so maps/filters/takes/reductions can fuse without intermediate allocation.

Example shape:

```forge
val xf = #forge/transducer {
    :input #type "u32"
    :steps [
        #forge/map [](x: u32) -> u32 { return x * 2; }
        #forge/filter [](x: u32) -> bool { return x > 100; }
        #forge/take 20
    ]
};
```

Exact reader payload syntax may live in the standard-library reader namespace and can evolve compatibly without changing core parser grammar.

---

# Part XIV — Intentionally absent from v1

Forge v1 deliberately excludes:

- user-defined parametric generics;
- class inheritance;
- mandatory runtime virtual dispatch;
- garbage collection;
- automatic reference counting;
- language exceptions;
- language-level `null`;
- C implicit numeric promotion;
- array-to-pointer decay;
- textual macro preprocessing;
- implicit switch fallthrough;
- arbitrary runtime reflection;
- automatic heap allocation for escaping closures;
- green threads/mandatory user-space scheduler.

Future versions may add facilities when they can be added without changing v1 source meaning.

---

# Part XV — Representative examples

## 67. Result + defer + explicit durable allocator

```forge
fn load_document(path: str, allocator: &Allocator)
    -> Result[Document, LoadError]
{
    val file = file_open(path)?;
    defer file_close(file);

    val bytes = file_read_all(file, allocator)?;
    return parse_document(bytes, allocator);
}
```

## 68. Temporary scratch memory

```forge
fn parse_header(bytes: u8[]) -> Result[Header, ParseError] {
    val mark = context.scratch.mark();
    defer context.scratch.release(mark);

    val tokens = header_tokens(bytes, context.scratch.allocator())?;
    return Header.parse(tokens);
}
```

No scratch allocation may escape in returned `Header` unless `Header` copies/owns it elsewhere.

## 69. Option without null

```forge
fn cache_find(cache: &Cache, key: Key) -> &Value? {
    ...
}

match (cache_find(&cache, key)) {
    Some{value} => use(value),
    None => recompute(),
}
```

## 70. Closure and higher-order operation

```forge
val factor: u32 = 3;
val triple = [factor](x: u32) -> u32 {
    return x * factor;
};

val result = apply_twice(4u32, triple);
```

## 71. Pattern-rich parser code

```forge
match (node) {
    Expr::Binary{
        op: Op::Add,
        left: Expr::Number{value: a},
        right: Expr::Number{value: b}
    } => Expr::Number{value: @check(a + b)},

    whole @ Expr::Neg{
        value: Expr::Number{value}
    } when value != MIN_I64 => fold_neg(whole, value),

    _ => node,
}
```

## 72. Named/default call

```forge
nfn open_window(
    title: str,
    width: u32 = 1280,
    height: u32 = 720,
    resizable: bool = true
) -> Result[Window, WindowError] {
    ...
}

val window = open_window(
    :title = "Debugger",
    :resizable = false
)?;
```

## 73. Fixed-size pool and ECS-style owner

```forge
struct Server {
    connections: ConnectionPool;
    clients: ClientStore;
    memory: ServerArena;
}

fn accept(server: &mut Server, socket: Socket)
    -> Result[ConnectionId, ServerError]
{
    val conn = server.connections.acquire()?;
    conn.socket = socket;
    return server.clients.attach(conn)?;
}
```

## 74. Unsafe device access

```forge
fn enable_device(base: usize) {
    unsafe {
        val control: *volatile u32 = ptr_from_address[*volatile u32](base + 0x10);
        volatile_store(control, 1u32);
    }
}
```

## 75. Metadata and reader values

```forge
@{
    :doc/title "Network connection"
    :since #version "1.0.0"
    :doc/see [#ref "std.net.Address"]
}
pub struct Connection {
    id: #forge/type-id #uuid "550e8400-e29b-41d4-a716-446655440000";
    ...
}
```

(Reader-tag use in type/initializer positions is legal only when that reader's expansion contract produces a construct valid in that position.)

---

# Part XVI — Implementation obligations

## 76. Diagnostics

Implementations must diagnose violations of:

- definite initialization;
- mutation rules;
- implicit conversion rules;
- unsafe-required operations;
- non-exhaustive closed matches;
- unreachable/invalid patterns where statically knowable;
- ambiguous overloads;
- invalid named arguments;
- closure escape beyond supported lifetime;
- invalid FDN/reader forms;
- duplicate metadata/map keys where prohibited.

## 77. Optimizer freedom

The compiler may remove checks or abstract operations only when it proves observable Forge semantics unchanged. In particular:

- a proven bounds check may disappear;
- a proven non-overflowing checked add may lower to plain machine add;
- a niche representation may remove explicit `Option` tags;
- a capture-free closure may become a direct function pointer;
- transducer stages may fuse into one loop.

The source language therefore pursues **zero-cost abstraction by proof and lowering**, not by making safety undefined or globally disabled.
