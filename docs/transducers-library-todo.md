# Forge transducers library TODO

Forge should provide transducers as a standard collection-processing abstraction. The goal is to express reusable transformation pipelines independently of the source collection and destination accumulator, while avoiding intermediate collections.

This is particularly useful before Forge has language-level generics: the transducer protocol can be generated into concrete, type-specialized families in the same way as the collections library.

The semantic reference is Clojure's transducer model: a transducer transforms a reducing function; reducers have initialization, step and completion behavior; stateful transducers allocate/reset reduction-local state; early termination is explicit and must propagate through nested reductions.

## Design principles

- [ ] No dependency on language-level generics.
- [ ] Generate concrete transducer/reducer families for actual type combinations used by a program or library.
- [ ] No type erasure or `*void` payloads in normal generated pipelines.
- [ ] Composition is only valid when the output type of one stage matches the input type of the next stage.
- [ ] Transducers do not own their input source or final destination.
- [ ] Pipelines do not allocate intermediate collections.
- [ ] Stateful transducers create fresh state per `transduce` invocation; state must never leak between executions.
- [ ] Early termination is part of the reducer protocol, not an exception/panic.
- [ ] Completion must run exactly once for a finite transduction.
- [ ] Freestanding-compatible operators must not depend on `std`.
- [ ] Operators requiring allocation take an explicit allocator or use a destination-provided allocator policy.

## Type-specialized protocol

Without generics, generate concrete reducer and transducer shapes from the participating types.

Conceptually:

```text
Reducer<Input, Acc>
    init() -> Acc
    step(Acc, Input) -> StepResult<Acc>
    complete(Acc) -> Acc

Transducer<Input, Output, Acc>
    transforms Reducer<Output, Acc>
           into Reducer<Input, Acc>
```

Generated Forge names might be:

```text
ReducerU64U64
ReducerStringU64
ReducerStringListString

MapU64ToU64
MapStringToU64
FilterString
TakeString
```

The generator should normally hide these long names behind module functions/build-generated aliases.

Example valid pipeline:

```text
String --filter--> String --map--> U64 --take--> U64
```

An invalid composition such as `MapStringToU64` followed by `FilterString` must fail at generation/check time.

## Core reduction machinery

- [ ] `Reduced<T>` / generated `ReducedU64`, etc., or equivalent `StepResult` representation.
- [ ] `continue(value)`.
- [ ] `reduced(value)`.
- [ ] `is_reduced`.
- [ ] `unreduced`.
- [ ] reducer init/step/completion contract.
- [ ] `transduce` over supported source iterators/collection walkers.
- [ ] `reduce` without a transformation.
- [ ] `into` adapters for generated lists, sets and maps.
- [ ] iterator/walker adapters for `List*`, `HashSet*`, `HashMap*`, fixed vectors and slices.
- [ ] source-independent tests proving the same pipeline works over at least two source kinds.

## Phase 1 — stateless transducers

These should be implemented first because they exercise composition without requiring per-run mutable state.

- [ ] `identity`
  - pass every item unchanged.
- [ ] `map`
  - transform exactly once per input item.
  - support same-type and cross-type generated forms.
- [ ] `filter`
  - pass matching items only.
- [ ] `remove`
  - inverse predicate filter.
- [ ] `keep`
  - mapping callback returns optional value; discard `None`.
- [ ] `cat`
  - flatten one reducible level.
  - propagate early termination out of nested reduction.
- [ ] `mapcat`
  - generated composition of `map` + `cat` where practical.
- [ ] `replace`
  - map selected values through a replacement table.

Required tests:

- [ ] identity preserves all values and ordering.
- [ ] map transforms every value once.
- [ ] filter may invoke downstream reducer zero times for an input.
- [ ] remove is inverse-filter equivalent.
- [ ] keep removes empty optional results.
- [ ] map can change type, e.g. `String -> U64`.
- [ ] cat emits zero, one or many downstream values per input.
- [ ] mapcat equals explicit `map` then `cat`.
- [ ] no intermediate destination collection is created during a composed reduction.

## Phase 2 — finite/early-termination transducers

- [ ] `take(n)`.
- [ ] `take_while(predicate)`.
- [ ] `take_nth(n)`.
- [ ] `halt_when(predicate)`.

Required tests:

- [ ] `take(0)` consumes/emits nothing.
- [ ] `take(n)` stops the upstream reduction immediately after n outputs.
- [ ] `take_while` stops on the first failed predicate and does not inspect later source values.
- [ ] `take_nth(1)` is identity-equivalent.
- [ ] `halt_when` propagates the chosen terminal result.
- [ ] early termination propagates through `cat`/nested reductions.
- [ ] completion still executes exactly once after early termination.

## Phase 3 — stateful filtering transducers

- [ ] `drop(n)`.
- [ ] `drop_while(predicate)`.
- [ ] `dedupe`.
- [ ] `distinct`.
- [ ] indexed counter primitive.
- [ ] `map_indexed`.
- [ ] `keep_indexed`.
- [ ] `interpose(separator)`.

Required tests:

- [ ] drop count resets for every transduction.
- [ ] drop_while permanently switches to pass-through after first failure.
- [ ] dedupe removes only adjacent duplicates.
- [ ] distinct removes duplicates across the entire reduction.
- [ ] distinct state does not leak across separate transductions.
- [ ] map_indexed starts at zero for each run.
- [ ] keep_indexed advances index even for discarded outputs.
- [ ] interpose never emits a leading or trailing separator.
- [ ] stateful transducer instances can be reused safely by creating fresh execution state.

`distinct` requires a set and therefore depends on the generated HashSet implementation for its key type. This is an intentional integration point with the collections library.

## Phase 4 — buffering/completion transducers

- [ ] `partition_all(n)`.
- [ ] `partition_by(key_fn)`.
- [ ] optional `scan` / running reduction.

Required tests:

- [ ] partition_all emits exact full partitions.
- [ ] partition_all flushes a short final partition during completion.
- [ ] partition_all does not flush pending state after a downstream early termination.
- [ ] partition_by starts a new partition only when the derived key changes.
- [ ] partition_by flushes its final partition exactly once.
- [ ] partition state is fresh for each transduction.
- [ ] allocator failure while growing a partition is surfaced explicitly.

## Phase 5 — diagnostics and utility operators

Useful but not required for the first collections/CKV milestone:

- [ ] `tap(callback)` for observing values without modifying the stream.
- [ ] `enumerate` alias/helper if `map_indexed` is awkward for common use.
- [ ] `scan` if not implemented in Phase 4.
- [ ] `chunk(n)` alias only if it has clearly different semantics from `partition_all`.

Deferred:

- [ ] `random_sample` — requires a randomness provider and makes deterministic bootstrap tests harder.
- [ ] time/window transducers — require clocks/scheduling and belong above the core collection layer.
- [ ] parallel transduction — only after deterministic sequential semantics are frozen.

## Composition tests

Composition is the main reason to build this library. These tests are mandatory.

### Stateless pipeline

Input:

```text
0 1 2 3 4 5 6 7 8 9
```

Pipeline:

```text
filter even
map x * 10
take 3
```

Expected:

```text
0 20 40
```

- [ ] result matches manually written loop.
- [ ] downstream reducer sees exactly 3 values.
- [ ] source walker stops after the value needed to satisfy `take`.

### Cross-type pipeline

Input `ListString`:

```text
"1" "20" "300" "bad" "4"
```

Pipeline:

```text
keep parse_u64 : String -> Option<U64>
filter >= 10
take 2
```

Expected:

```text
20 300
```

- [ ] proves generated cross-type composition works without generics.

### Stateful pipeline

Input:

```text
1 1 2 2 2 3 1 1 4
```

Pipeline:

```text
dedupe
filter odd
interpose 0
```

Expected:

```text
1 0 3 0 1
```

- [ ] repeat the same transduction twice and verify identical output.

### Buffered pipeline

Input:

```text
1 2 3 4 5
```

Pipeline:

```text
map x * 2
partition_all 2
```

Expected partitions:

```text
[2 4]
[6 8]
[10]
```

- [ ] proves completion flush semantics.

### Collection-independent pipeline

Use the same `filter -> map -> take` transformation over:

- [ ] generated `ListU64`.
- [ ] fixed-capacity `FixedVecU64`.
- [ ] a simple source callback/walker.

All must produce identical output without changing the transducer implementation.

## Generated-code strategy

- [ ] Add transducer generation alongside collection generation rather than a second unrelated generator.
- [ ] Describe required pipelines/type transitions in `forge.fdn` or a generated-code declaration file.
- [ ] Deduplicate generated reducer/transducer specializations across a package graph.
- [ ] Generate only combinations actually requested by packages plus standard-library bootstrap combinations.
- [ ] Generated artifacts must be deterministic.
- [ ] Generated artifacts participate in normal Forge checking/tests.

Bootstrap specializations likely required first:

```text
U8 -> U8
U32 -> U32
U64 -> U64
Usize -> Usize
String -> String
String -> U64
U64 -> String
```

Accumulator/destination specializations initially needed:

```text
ListU64
ListString
HashSetU64
HashSetString
HashMapStringString entry walker
scalar reducers: count, sum, min, max
```

## Reducers / terminal operations

Transducers are most useful if terminal reducers are also standardized.

- [ ] `count`.
- [ ] `sum_u64`, signed and other numeric generated forms.
- [ ] `min` / `max`.
- [ ] `first` with early termination.
- [ ] `last`.
- [ ] `any` / `all`.
- [ ] `find` with early termination.
- [ ] `collect_list`.
- [ ] `collect_set`.
- [ ] map-entry collection where key/value output types permit it.

## Integration milestones

- [ ] Executable semantic/reference tests for every operator before Forge implementation is considered complete.
- [ ] Forge implementation of reducer protocol.
- [ ] Forge implementation of map/filter/take and composition.
- [ ] Generated specialized pipeline passes CForge JVM tests.
- [ ] Same pipeline passes CForge native-image tests.
- [ ] Replace ad-hoc CKV full-file record loop with a transducer pipeline once `String -> record` parsing shape is clean.
- [ ] Use transducers with generated `HashMapStringString` rather than the bootstrap host map.
- [ ] Exercise a freestanding pipeline in Cosmic/kernel-oriented tests with no `std` dependency.

## Non-goals

The first implementation does not need lazy sequences, coroutines, asynchronous streams, parallel evaluation, dynamic typing, reflection, or a universal boxed iterator type. The important property is source/destination independence and composition without intermediate collections.