# CKV integration application

CKV is the first non-trivial hosted Forge application intended to exercise the compiler, build system, standard library, runtime providers, persistence, locking, timing and collections together.

The initial implementation is deliberately simple. There is no on-disk index. Every command opens/locks the database, reads the complete file, rebuilds an in-memory hash map, performs a normal hash lookup/update, and then releases the lock.

## Initial command surface

```text
ckv --database-file dbfile.db set test 5
ckv --database-file dbfile.db get test
5
access: 123 us
```

Initial commands:

- [x] `set KEY VALUE`
- [x] `get KEY`
- [ ] `delete KEY`
- [ ] `list`
- [ ] `count`
- [ ] `verify`
- [ ] `compact`

## Initial file format

Bootstrap format:

```text
KEY<TAB>VALUE<LF>
```

The complete file is an append log. Loading applies records from first to last, so the latest value for a key wins. A later binary CKV format will replace this once byte buffers, binary file I/O and checksums are executable Forge library code.

This is intentionally not an indexed database. The first goal is integration coverage, not storage efficiency.

## Canonical application and bootstrap boundary

The application algorithm is Forge source, not Clojure:

```text
examples/ckv/
├── forge.fdn
├── src/main.fg
└── packages/
    └── ckv-core/
        ├── forge.fdn
        └── src/lib.fg
```

`src/main.fg` contains argument handling, console output and timing. `ckv-core/src/lib.fg` contains the database algorithm. `ckv-core` is a real local-path dependency resolved by the Forge build system.

For CForge execution only, `ckv-core` currently imports the explicitly named `forge-collections-bootstrap` package. That package uses CForge raw-storage handles and is not the production collection ABI.

The production collection package is `forge-collections-native`. Its ordinary collection values never retain allocator capabilities. Operations that allocate/grow/free receive an explicit `core.Allocator` argument.

The acceptance path is therefore:

```text
current bootstrap:
CKV Forge algorithm
    -> forge-collections-bootstrap
    -> CForge raw storage emulator

final/native:
CKV Forge algorithm
    -> forge-collections-native HashMapStringString
    -> explicit allocator argument on create/grow/put/destroy paths
    -> core.Allocator / MemoryBlock
    -> hosted or Cosmic provider
```

The older CForge CKV implementation remains only a reference/oracle for differential/provider testing.

## Implementation stages

### Stage 0 — reference behavior

- [x] Full-file load on every operation.
- [x] Build a normal in-memory hash map from the loaded records.
- [x] Latest record wins.
- [x] Append a SET record on mutation.
- [x] Measure each access using a monotonic clock.
- [x] Hold a file lock across load + operation + persistence.
- [x] Make file, lock, clock and bootstrap map implementations replaceable providers in CForge.
- [x] Test provider substitution and lock release on exceptions.

### Stage 1 — hosted Forge APIs required by CKV

- [x] `std.args` bootstrap API and program-argument forwarding.
- [x] `std.fs` bootstrap text read/write/append provider.
- [x] `std.lock` bootstrap exclusive file lock.
- [x] `std.time` monotonic microsecond clock.
- [x] `std.string` CKV parsing/formatting helpers.
- [x] `forge-collections-bootstrap` provides executable CForge hash-map semantics.
- [x] legacy `std.collections.string_map` production-facing file removed.
- [x] ambiguous `forge-collections` raw-handle package removed.
- [ ] migrate portable string helpers from runtime hooks into Forge.
- [ ] native Unix/Cosmic providers for hosted services.

`std.fs`, `std.lock`, `std.time` and `std.args` legitimately terminate in platform providers. Collections do not: the final hash table algorithm and ownership model are Forge code over explicit allocator/raw-memory primitives.

### Stage 2 — Forge CKV program

- [x] `examples/ckv` is a Forge package with `forge.fdn`.
- [x] Database logic lives in reusable local-path package `ckv-core`.
- [x] CLI imports `ckv-core` plus hosted `std` modules.
- [x] CForge supports real local and cross-package function calls with locals/loops.
- [x] `forge run ... -- ARGS...` forwards application arguments.
- [x] CForge runs the canonical Forge CKV algorithm on the JVM through `forge run`.
- [x] End-to-end CI performs separate `set`/`get` invocations against a real file.
- [x] Exact value output is asserted while timing output is pattern-checked.
- [x] Overwrite is tested and the append log is verified.
- [x] CForge native image runs the same CKV source.

### Stage 2b — final collection migration

Before CKV is considered a native collection acceptance test:

- [ ] create/select a hosted `core.Allocator` explicitly in CKV/application startup.
- [ ] pass the allocator explicitly to `HashMapStringString` operations that allocate/grow/free.
- [ ] do not store the allocator inside `HashMapStringString` or another ordinary wrapper.
- [ ] preserve allocator-domain provenance through the lifetime of each map.
- [ ] destroy/free map storage using the same or explicitly compatible allocator domain.
- [ ] run unchanged persistence behavior through production Forge backend.

### Stage 3 — stronger persistence semantics

- [ ] Missing-key result distinct from an empty value in the Forge API.
- [ ] Reject tabs/newlines in user keys/values before persistence.
- [ ] Detect malformed bootstrap records rather than silently skipping them.
- [ ] `delete` tombstones.
- [ ] `compact` rewrites one current record per live key.
- [ ] Temporary-file + atomic replace for compaction.
- [ ] Flush/sync policy documented.
- [ ] Crash/truncated-tail behavior specified and tested.
- [ ] Binary file format with magic/version/record length.
- [ ] CRC32 record integrity.

### Stage 4 — portability gate

- [ ] Native Unix Forge platform provider.
- [x] CForge hosted reference provider on JVM and GraalVM native image.
- [ ] Cosmic userspace provider.
- [ ] Same `ckv-core` database semantics pass persistence tests on native Unix Forge and Cosmic using explicit allocator calls.

## Architecture

```text
ckv CLI (Forge)
   |
   v
ckv-core (Forge local package)
   |
   +-- current: forge-collections-bootstrap
   +-- target:  forge-collections-native + explicit Allocator
   +-- std.string
   +-- std.fs
   +-- std.lock
   +-- std.time
   |
   v
platform/provider boundary only for actual platform services
```

`ckv-core` must not call Java, Unix, CForge or Cosmic APIs directly. File, lock, clock and process-argument behavior belongs behind hosted platform providers. Collection allocation is not a hidden platform service: native CKV passes its allocator explicitly.

## Integration tests

- [x] hosted service/provider unit tests
- [x] Forge source calls hosted std APIs
- [x] real local and cross-package function-call tests
- [x] create/set/get persistence using canonical Forge CKV algorithm
- [x] overwrite a key and observe the latest value
- [x] every CKV command reloads the complete file by design
- [x] real file locking exercised by Forge source
- [x] access timing produced by monotonic provider
- [x] separate process invocations for set/get in CI
- [x] same CKV sequence through GraalVM-native CForge
- [ ] final explicit-allocator HashMapStringString path
- [ ] allocation failure injection on map growth/rehash
- [ ] wrong allocator-domain detection in debug/reference provider
- [ ] simultaneous writers serialize correctly
- [ ] shared-reader/exclusive-writer policy
- [ ] large database (10k+ keys)
- [ ] hash collision stress
- [ ] truncated final record recovery
- [ ] native Forge compiler execution
- [ ] Cosmic execution
