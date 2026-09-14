# CKV integration application

CKV is the first non-trivial hosted Forge application intended to exercise the compiler, build system, standard library, runtime providers, persistence, locking, timing, allocation and collections together.

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

## Canonical implementation

The application is Forge source, not Clojure:

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

The older CForge CKV implementation is only a reference/oracle used for differential and provider testing. Application behavior must be implemented in Forge source.

## Implementation stages

### Stage 0 — reference behavior

- [x] Full-file load on every operation.
- [x] Build a normal in-memory hash map from the loaded records.
- [x] Latest record wins.
- [x] Append a SET record on mutation.
- [x] Measure each access using a monotonic clock.
- [x] Hold a file lock across load + operation + persistence.
- [x] Make file, lock, clock and map implementations replaceable providers in CForge.
- [x] Test provider substitution and lock release on exceptions.

### Stage 1 — hosted Forge APIs required by CKV

- [x] `std.args` bootstrap API
  - [x] argument count
  - [x] indexed argument access
  - [x] `--` program-argument forwarding through `forge run`
  - [x] provider/bounds tests
- [x] `std.fs` bootstrap text API
  - [x] read whole text file
  - [x] write whole text file
  - [x] append text
  - [x] real filesystem/provider tests
  - [ ] explicit flush/sync semantics
  - [ ] structured filesystem errors/metadata/open handles
- [x] `std.lock` bootstrap API
  - [x] exclusive file lock
  - [x] real CForge/Unix-host lock provider
  - [x] provider substitution/release tests
  - [ ] release via Forge `defer` on every error path
  - [ ] two-process exclusion test
- [x] `std.time` bootstrap API
  - [x] monotonic microsecond timestamp
  - [x] elapsed-time arithmetic in Forge
  - [x] provider substitution tests
  - [ ] richer `Instant`/`Duration` types
- [x] `std.string` CKV bootstrap helpers
  - [x] concatenation
  - [x] line iteration helpers
  - [x] tab delimiter parsing
  - [x] integer-to-string formatting for timing
  - [ ] migrate portable operations from runtime hooks into Forge implementations
  - [ ] general UTF-8/string API
- [x] `std.collections.string_map` bootstrap API
  - [x] create
  - [x] put/replace
  - [x] contains/get
  - [x] count
  - [x] provider behavior tests
  - [ ] implement the hash table itself in Forge over allocator/array primitives
  - [ ] collision/growth/allocation-failure tests for the eventual Forge implementation

`std.fs`, `std.lock`, `std.time` and `std.args` are expected to terminate in platform providers. The current `StringMap` provider is a bootstrap implementation only; collections should eventually be portable Forge code rather than host-provided behavior.

### Stage 2 — real Forge CKV program

- [x] `examples/ckv` is a Forge package with `forge.fdn`.
- [x] Database logic lives in reusable local-path package `ckv-core`.
- [x] CLI imports `ckv-core` plus hosted `std` modules.
- [x] CForge supports real local and cross-package function calls with locals/loops.
- [x] `forge run ... -- ARGS...` forwards application arguments.
- [x] CForge runs the canonical Forge CKV program on the JVM through `forge run`.
- [x] End-to-end CI performs separate `set`/`get` invocations against a real file.
- [x] Exact value output is asserted while timing output is pattern-checked.
- [x] Overwrite is tested and the append log is verified to contain two records.
- [ ] CForge native image runs exactly the same package and source. (CI gate added; awaiting/maintaining green native run.)

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
- [x] CForge hosted reference provider.
- [ ] Cosmic userspace provider.
- [ ] Same `ckv-core` source passes persistence tests on native Unix Forge and Cosmic.

## Architecture

```text
ckv CLI (Forge)
   |
   v
ckv-core (Forge local package)
   |
   +-- std.collections.string_map
   +-- std.string
   +-- std.fs
   +-- std.lock
   +-- std.time
   |
   v
std platform/provider boundary
   |                    |
 CForge/Unix host      Cosmic
```

`ckv-core` must not call Java, Unix, CForge or Cosmic APIs directly. Environment-specific file, lock, clock and process-argument behavior belongs behind the standard-library platform-provider boundary.

## Integration tests

The CKV suite is intended to become one of the required Forge/Cosmic integration gates:

- [x] hosted service/provider unit tests
- [x] Forge source calls the hosted std APIs
- [x] real local and cross-package function-call tests
- [x] create/set/get persistence using canonical Forge CKV
- [x] overwrite a key and observe the latest value
- [x] every CKV command reloads the complete file by design
- [x] real file locking exercised by Forge source
- [x] access timing produced by the monotonic provider
- [x] separate process invocations for set/get in CI
- [ ] simultaneous writers serialize correctly
- [ ] shared-reader/exclusive-writer policy
- [ ] large database (10k+ keys)
- [ ] hash collision stress once the map itself is Forge code
- [ ] allocator failure injection once the map itself is Forge code
- [ ] truncated final record recovery
- [ ] native Forge compiler execution
- [ ] Cosmic execution
