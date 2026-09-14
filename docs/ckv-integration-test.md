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

- [x] `set KEY VALUE` semantics defined
- [x] `get KEY` semantics defined
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

The complete file is an append log. Loading applies records from first to last, so the latest value for a key wins. The bootstrap format rejects tabs/newlines inside keys and values. A later binary CKV format will replace this once byte buffers, binary file I/O and checksums are executable Forge library code.

This is intentionally not an indexed database. The first goal is integration coverage, not storage efficiency.

## Implementation stages

### Stage 0 — reference behavior

- [x] Full-file load on every operation.
- [x] Build a normal in-memory hash map from the loaded records.
- [x] Latest record wins.
- [x] Append a SET record on mutation.
- [x] Missing key is distinct from an empty value.
- [x] Reject malformed records rather than silently accepting corruption.
- [x] Measure each access using a monotonic clock.
- [x] Hold a file lock across load + operation + persistence.
- [x] Make file, lock, clock and map implementations replaceable providers in CForge.
- [x] Test provider substitution and lock release on exceptions.

### Stage 1 — hosted Forge APIs required by CKV

- [ ] `std.args`
  - [ ] argument count
  - [ ] indexed argument access
  - [ ] `--` program-argument forwarding through `forge run`
  - [ ] bounds/error tests
- [ ] `std.fs`
  - [ ] exists/open/read-all text
  - [ ] append text
  - [ ] explicit flush semantics
  - [ ] missing-file/error tests
- [ ] `std.lock`
  - [ ] exclusive file lock
  - [ ] release via `defer`
  - [ ] Unix provider
  - [ ] provider substitution tests
  - [ ] two-process exclusion test
- [ ] `std.time`
  - [ ] monotonic timestamp
  - [ ] duration arithmetic
  - [ ] provider substitution tests
- [ ] `std.string`
  - [ ] length
  - [ ] slice
  - [ ] search for byte/delimiter
  - [ ] integer formatting for access timing
  - [ ] UTF-8/byte-boundary tests for whichever operations are byte-oriented
- [ ] `std.collections.map`
  - [ ] create mutable map with explicit allocator
  - [ ] put/replace
  - [ ] contains/get
  - [ ] count
  - [ ] collision tests
  - [ ] growth tests
  - [ ] allocation-failure tests

### Stage 2 — real Forge CKV program

- [ ] `examples/ckv` becomes a Forge package with `forge.fdn`.
- [ ] Split database logic into a reusable `ckv-core` local-path library package.
- [ ] CLI application imports `ckv-core` plus hosted `std` modules.
- [ ] CForge runs the program on the JVM through `forge run`.
- [ ] CForge native image runs exactly the same package and source.
- [ ] End-to-end subprocess tests execute `set`, then a separate `get` process.
- [ ] Exact value output is asserted while timing output is pattern-checked.

### Stage 3 — stronger persistence semantics

- [ ] `delete` tombstones.
- [ ] `compact` rewrites one current record per live key.
- [ ] Temporary-file + atomic replace for compaction.
- [ ] Flush/sync policy documented.
- [ ] Crash/truncated-tail behavior specified and tested.
- [ ] Binary file format with magic/version/record length.
- [ ] CRC32 record integrity.

### Stage 4 — portability gate

- [ ] Unix standard-library provider.
- [ ] CForge reference provider.
- [ ] Cosmic userspace provider.
- [ ] Same `ckv-core` source passes persistence tests on Unix and Cosmic.

## Architecture

```text
ckv CLI
   |
   v
ckv-core
   |
   +-- std.collections.map
   +-- std.string
   +-- std.fs
   +-- std.lock
   +-- std.time
   |
   v
std platform provider
   |             |
 Unix          Cosmic
```

`ckv-core` must not call Unix APIs directly. Environment-specific file, lock and clock behavior belongs behind the standard-library platform-provider boundary.

## Integration tests

The CKV suite should eventually be one of the required Forge/Cosmic integration gates:

- [x] create/set/get persistence in the CForge reference implementation
- [x] overwrite a key and observe the latest value
- [x] empty value remains distinguishable from absent key
- [x] each operation performs a full-file reload
- [x] malformed record fails explicitly
- [x] lock is released after errors
- [x] access timing is produced from the monotonic provider
- [ ] separate-process set/get
- [ ] simultaneous writers serialize correctly
- [ ] reader/writer locking policy
- [ ] large database (10k+ keys)
- [ ] hash collision stress
- [ ] allocator failure injection
- [ ] truncated final record recovery
- [ ] native Forge compiler execution
- [ ] Cosmic execution
