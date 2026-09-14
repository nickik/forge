# Forge standard library TODO

This document tracks the staged standard-library work needed before and during Cosmic OS development.

The library model is:

```text
language/runtime intrinsics
        |
       core
        |
  core.* freestanding companions
        |
       std
        |
 std.platform provider layer
        |
 Unix / Cosmic / other hosted environments
```

`core` and `core.*` must remain usable without an operating system. `std` is hosted and may depend on a selected platform provider. None of these layers requires traits or interfaces: runtime-pluggable resources use explicit `context + Ops` capability objects, while process-global hosted services use build-selected platform providers.

## Phase 0 — package/module foundation

- [x] `forge.fdn` package manifest.
- [x] Local path dependency graph.
- [x] Deterministic dependency order and cycle rejection.
- [x] `library`, `executable`, `kernel`, and `test` targets.
- [x] Build-driver protocol for `check`, `run`, and tests.
- [x] Pass dependency library roots to compiler/interpreter drivers.
- [x] General local/cross-package function-call execution in CForge; imported functions may contain locals, loops and calls.
- [x] `forge run ... -- ARGS...` application argument forwarding.
- [ ] Multiple source modules per package/library target.
- [ ] Package-private versus public module visibility rules.
- [ ] Library interface serialization/cache for incremental builds.
- [ ] Workspace manifests.
- [ ] `forge.lock` for exact dependency graphs.

## Phase 1 — `core`

`core` is always available in freestanding builds and may not assume an OS, libc, process heap, filesystem, scheduler, environment variables, or normal program entry point.

### Language-adjacent values

- [x] `Option`/`Result` remain language-defined bootstrap types.
- [ ] Stable helpers for `Option` and `Result`.
- [ ] Ordering/comparison helper types where needed.
- [ ] Compile-time target/layout facts.

### Panic and failure

- [x] `PanicKind`, `PanicInfo`, and non-returning panic provider contract.
- [x] Freestanding program supplies panic implementation; hosted `std` supplies default.
- [ ] Assertion helpers.
- [ ] Explicit unreachable/abort helpers.
- [ ] Panic formatting that does not allocate.

### Allocation

- [x] `Arena` capability (`context + ArenaOps`).
- [x] `Allocator` capability for arbitrary-size allocations.
- [x] `ObjectCache` model for fixed-size objects.
- [x] Kernel/user provider-equivalence tests.
- [ ] Forge implementation of slab/object-cache allocation.
- [ ] General size-class allocator layered on object caches.
- [ ] Reclaim hooks and pressure accounting.
- [ ] Optional magazines/per-CPU caches without changing the API.
- [ ] Debug modes: poisoning, red zones, duplicate-free detection.

## Phase 2 — freestanding `core.*` libraries

### `core.mem`

- [ ] `copy_nonoverlapping`.
- [ ] `move_overlapping`.
- [ ] `set_bytes` / zeroing.
- [ ] byte comparison.
- [ ] checked byte-span helpers.
- [ ] optimized target-specific implementations behind the same API.

### `core.bits`

- [x] Initial mask/field/endian reference behavior.
- [ ] byte swaps for all integer widths.
- [ ] LE/BE load/store helpers.
- [ ] rotate left/right.
- [ ] popcount.
- [ ] leading/trailing-zero count.
- [ ] safe field extract/insert helpers.

### `core.mmio`

- [x] Volatile load/store contract.
- [ ] Compiler-backed volatile primitives.
- [ ] typed register wrappers.
- [ ] read-only/write-only/read-write register forms.
- [ ] ordering/fence documentation for devices.

### `core.atomic`

- [x] Memory-ordering model and reference behavior.
- [ ] `AtomicU8/U16/U32/U64/Usize`.
- [ ] load/store/exchange.
- [ ] compare-exchange.
- [ ] fetch add/sub/and/or/xor.
- [ ] compiler and memory fences.
- [ ] target capability checks for unsupported atomic widths.

### `core.sync`

- [x] Spin-lock/critical-section capability model.
- [ ] production `SpinLock` on Forge atomics.
- [ ] one-time initialization primitive.
- [ ] interrupt/critical-section provider contract.
- [ ] no scheduler-aware mutexes here; those belong to the OS.

### `core.fixed`

- [x] Reference bounded-vector and ring-buffer behavior.
- [ ] generated `FixedVec_T_N`.
- [ ] generated `RingBuffer_T_N`.
- [ ] fixed-capacity string/byte builder.
- [ ] iteration helpers without allocation.
- [ ] insertion/removal boundary tests.

### `core.intrusive`

- [x] Reference intrusive FIFO behavior.
- [ ] intrusive doubly linked list.
- [ ] intrusive queue/deque.
- [ ] ownership/link-state checks in debug builds.
- [ ] patterns suitable for schedulers, VM page lists, IPC waiters and slab lists.

### `core.layout`

- [x] alignment arithmetic/reference behavior.
- [ ] `sizeof`.
- [ ] `alignof`.
- [ ] `offsetof`.
- [ ] compile-time/static assertions.
- [ ] reliable `@repr(c)`, packed and explicit alignment support.

### `core.io`

- [x] `Writer` capability design.
- [x] non-allocating reference formatting.
- [ ] byte/string write operations.
- [ ] integer decimal formatting.
- [ ] hexadecimal/pointer formatting.
- [ ] fixed-buffer writer.
- [ ] serial/framebuffer/memory-log providers supplied outside `core`.

### `core.target`

- [x] target-facts contract.
- [ ] architecture.
- [ ] pointer width.
- [ ] endian.
- [ ] ABI/environment identifier.
- [ ] atomic-width capabilities.
- [ ] page-size remains platform/OS policy rather than a universal target fact.

## Phase 3 — minimal hosted `std`

### `std.console`

- [x] portable `write(str)` facade.
- [x] replaceable CForge host provider.
- [ ] Unix platform provider implemented through the Forge native/platform ABI.
- [ ] Cosmic userspace provider.
- [ ] stderr equivalent.
- [ ] line-oriented helpers.
- [ ] formatting layered on `core.io` rather than a separate formatter.

### `std.args` / `std.process`

- [x] bootstrap argument count/index access API.
- [x] CForge hosted provider and Forge-level tests.
- [x] build-system forwarding for `forge run ... -- ARGS...`.
- [ ] process exit.
- [ ] environment access.
- [ ] current working directory only when filesystem layer exists.
- [ ] eventual consolidation/naming decision between `std.args` and `std.process.args`.

### `std.time`

- [x] bootstrap monotonic microsecond clock API.
- [x] CForge provider and deterministic provider tests.
- [ ] `Instant` and `Duration` value types.
- [ ] wall-clock time.
- [ ] sleep belongs here only for hosted environments.
- [ ] native Unix and Cosmic providers.

### `std.fs`

- [x] bootstrap whole-file text read/write/append API.
- [x] CForge real-filesystem/provider tests.
- [ ] structured filesystem errors.
- [ ] file handles/open/close/read/write.
- [ ] flush/sync semantics.
- [ ] metadata/stat.
- [ ] directories.
- [ ] path manipulation kept separate from actual filesystem I/O where practical.
- [ ] buffered I/O after primitive file operations are stable.

### `std.lock`

- [x] bootstrap exclusive file-lock facade.
- [x] real hosted CForge lock using an OS file lock.
- [x] provider substitution/release tests.
- [ ] shared locks.
- [ ] `defer`-safe lock guard/release convention.
- [ ] two-process exclusion tests.
- [ ] native Unix and Cosmic implementations.

### `std.string`

- [x] CKV bootstrap helpers for concatenation, line parsing, delimiter parsing and integer formatting.
- [x] Forge-level behavior tests through CForge.
- [ ] move portable operations out of runtime hooks and into Forge code.
- [ ] byte length/slicing/search.
- [ ] dynamic owning `String` once allocator-backed collections are ready.
- [ ] documented UTF-8 versus byte-oriented operation semantics.

### `std.collections`

Detailed collection work is tracked in `docs/collections-library-todo.md`.

- [x] bootstrap `std.collections.string_map` API used by CKV.
- [ ] generated `List*` families.
- [ ] generated `HashSet*` families.
- [ ] generated `HashMap*` families.
- [ ] explicit allocator integration.
- [ ] collision/growth/removal/allocation-failure tests.
- [ ] replace CKV bootstrap host map with Forge `HashMapStringString`.

### `std.transducers`

Detailed transducer work is tracked in `docs/transducers-library-todo.md`.

Purpose: provide source/destination-independent transformation pipelines without intermediate collections, and partly compensate for the absence of language-level generics by generating concrete typed reducer/transducer families.

- [ ] reducer init/step/completion protocol.
- [ ] explicit reduced/early-termination representation.
- [ ] generated/type-specialized transducer composition.
- [ ] `transduce`, `reduce`, and `into` adapters.
- [ ] stateless: `identity`, `map`, `filter`, `remove`, `keep`, `cat`, `mapcat`, `replace`.
- [ ] finite/early stop: `take`, `take_while`, `take_nth`, `halt_when`.
- [ ] stateful: `drop`, `drop_while`, `dedupe`, `distinct`, `map_indexed`, `keep_indexed`, `interpose`.
- [ ] buffered/completion: `partition_all`, `partition_by`; consider `scan`.
- [ ] utility/debug: `tap` after the core semantics are stable.
- [ ] terminal reducers: count, sum, min/max, first/last, any/all, find, collect-list/set/map.
- [ ] same typed pipeline works over generated lists, fixed vectors and source walkers.
- [ ] reference semantic contract tests green before Forge implementation is accepted.
- [ ] CForge JVM and native-image execution tests for generated Forge pipelines.
- [ ] later use CKV record loading as a real transducer integration test.

### `std.net`

- [ ] socket-like byte-stream/datagram abstraction for hosted systems.
- [ ] address parsing/formatting.
- [ ] DNS/service discovery belongs above primitive networking.
- [ ] Cosmic provider should map cleanly onto Cosmic/GNet services rather than require Unix internals.

### `std.thread`

- [ ] hosted thread creation/join.
- [ ] scheduler-aware mutex/condition primitives.
- [ ] thread-local storage if required.
- [ ] keep atomics/spin primitives in `core`.

### `std.alloc`

- [ ] hosted process allocator provider.
- [ ] default/global allocator policy for hosted applications.
- [ ] Unix VM-backed Arena.
- [ ] Cosmic userspace VM-backed Arena.
- [ ] reuse the same `core` allocator/object-cache algorithms used by the kernel.

## Phase 4 — common higher-level libraries

These should not block Cosmic kernel bring-up.

- [ ] dynamic `String` and byte buffers using explicit/default allocator policy.
- [ ] dynamic vector/list.
- [ ] general hash map/set via generated collection families.
- [ ] ordered map/set if justified.
- [ ] sorting/search algorithms.
- [ ] hashing/checksum primitives.
- [ ] text/encoding helpers beyond the bootstrap CKV helpers.
- [ ] parsers/serialization helpers.
- [ ] random/entropy facade in hosted `std`; deterministic PRNG can be freestanding.

## Phase 5 — Cosmic integration gates

Before serious Cosmic implementation depends on Forge:

- [x] panic contract tested in freestanding mode.
- [x] Arena/Allocator/ObjectCache provider model tested with kernel-like and hosted-like providers.
- [x] freestanding library integration scenario.
- [x] local Forge package graph drives CForge tests.
- [x] cross-package public symbol import exercised by a runnable application.
- [x] general cross-package function-call execution in CForge, including locals/loops.
- [x] non-trivial Forge CKV application exercises local package, args, files, lock, clock, strings and map on hosted CForge/JVM.
- [x] same CKV gate green through CForge native image.
- [ ] Forge `HashMapStringString` replaces CKV bootstrap host map.
- [ ] at least one generated transducer pipeline executes on CForge JVM + native image and two source collection kinds.
- [ ] raw memory/compiler primitives execute through native Forge backend.
- [ ] MMIO and atomics validated in a freestanding/QEMU target.
- [ ] non-allocating panic/debug Writer works on a QEMU serial console.
- [ ] `core` allocator runs on a Cosmic page-backed Arena in QEMU.
- [ ] same allocator runs through hosted Unix/Cosmic-user providers.
- [ ] package-level ABI library shared by kernel and userspace.

## Explicit non-goals for the bootstrap standard library

Do not delay OS work for a complete general-purpose standard library. In particular, full Unicode, GUI, HTTP, TLS, database clients, rich async runtimes, package registries, reflection frameworks, and large collection ecosystems are post-bootstrap work.
