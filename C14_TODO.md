# C14 — full Forge v1 compatibility and default native toolchain

Goal: make the production Rust Forge compiler and `forge` build system the default implementation for the complete current Forge v1 language surface. CForge remains an independent semantic oracle, not the normal runtime. Forge v1 documentation, conformance fixtures, compiler semantics, native execution, standard-library providers and package/build behavior must describe and implement one language.

The live construct-by-construct implementation audit is maintained in
`docs/c14-completeness-matrix.md`.

## Normative basis

C14 is governed by:

- `docs/forge-v1-spec.md`;
- `docs/forge-v1-syntax-decisions.md`, which is currently a normative clarification and wins over older conflicting examples in the main spec;
- `docs/forge-v1-library-spec.md`;
- `docs/runtime-abi.md`;
- `docs/fdn-v1-spec.md`;
- `docs/compatibility.md`.

C14 must mechanically reconcile the syntax clarification back into the main language spec. It must not implement stale examples that the clarification explicitly removed from v1.

## Invariants

- `val` is immutable and `var` is mutable; every binding has an initializer.
- assignment is a statement; compound assignment is absent in v1.
- no `null`, user generics, implicit numeric promotion, `switch`, C ABI syntax, or C varargs.
- checked arithmetic/indexing remains the default.
- raw pointer operations remain explicit and unsafe.
- capture-free closures omit `[]`; captured closures use an explicit capture list.
- v1 metadata is prefix-only.
- the source-level module/library model remains independent of filesystem layout.
- hosted `std` and freestanding `core` keep their specified separation.
- the C9 layout/ABI remains the representation authority.
- no test-specific compiler exceptions: failures are classified and fixed at the language/runtime/build-system layer.

## C14a — one executable Forge v1 specification

- [ ] reconcile every stale Forge example in `docs/forge-v1-spec.md` with `docs/forge-v1-syntax-decisions.md`;
- [ ] reconcile compatibility/build documentation that still describes deferred v1 syntax such as `extern "C"`;
- [ ] distinguish intentionally invalid examples from valid Forge examples unambiguously;
- [ ] add a spec-example corpus that maps valid language examples to parse/check/run acceptance cases;
- [ ] ensure every complete Forge program presented as valid by the v1 spec parses and checks with `forgec`;
- [ ] ensure executable spec examples run with the documented result;
- [ ] make documentation drift detectable in CI rather than relying on manual audits.

## C14b — full frontend/static-semantics compatibility

Promote the complete accepted v1 surface through parser, HIR, type checking and FIR rather than stopping at parse-only coverage.

- [ ] declarations, aliases/distinct types, structs, enums, tagged unions and bitstructs;
- [ ] `val`/`var`/`const`, assignment and definite binding semantics;
- [ ] all integer/float/char/string/array/slice/pointer/reference/function types in v1;
- [ ] strict conversions, overload/named-argument rules and method-call resolution;
- [ ] `if`, `while`, both `for` forms, `break`, `continue`, `return`, `defer`, `unsafe`;
- [ ] `Option`, `Result`, `?`, qualified variants and constructors;
- [ ] complete v1 `match` semantics including exhaustiveness/unreachable checking, ranges, OR/as patterns, destructuring and guards;
- [ ] closures and capture rules supported by v1;
- [ ] `nfn`, context/select/agent/thread language surfaces that are normative in v1;
- [ ] generic structured metadata/readers where the spec requires preservation/validation;
- [ ] every syntax-negative and semantic-negative conformance case remains rejected for the specified reason.

## C14c — full native execution compatibility

Every v1 construct with runtime semantics must lower through FIR/ABI/Cranelift and execute correctly on the current hosted native target.

- [ ] expand native conformance beyond the current arithmetic/reference subset to all executable v1 constructs;
- [ ] keep checked overflow, divide-by-zero, shift, bounds and panic behavior aligned with the spec;
- [ ] execute structs/enums/tagged unions/Option/Result/pattern decisions with authoritative C9 layouts;
- [ ] execute closures/function pointers/method calls and control-flow cleanup correctly;
- [ ] execute globals/static initialization and all currently shipped portable `core` algorithms;
- [ ] implement the complete currently shipped hosted provider surface (`console`, `args`, `time`, `fs`, `lock`, `string` and collection bootstrap stores);
- [ ] run Game of Life unchanged with exact expected output;
- [ ] run CKV through the production compiler and hosted providers;
- [ ] keep CForge differential coverage for cases both implementations support.

## C14d — complete current `forge` build-system support

The manifest semantics already documented by the repository must work with `forgec` as the default driver.

- [ ] `forge check` performs semantic/type checking without requiring an executable `main` for library targets;
- [ ] `forge build` emits a real artifact instead of aliasing `check`;
- [ ] `forge run` executes hosted executable/test targets and forwards arguments after `--`;
- [ ] `forge test` runs runnable targets, honors byte-exact `:test {:expected ...}`, and does not try to execute libraries;
- [ ] local path dependency graphs pass all dependency library roots to the compiler deterministically;
- [ ] shipped `core` is available to every target and shipped `std` modules are available only when `:std true`;
- [ ] kernel `:std false` behavior is enforced rather than merely recorded;
- [ ] `:entry` is honored for nonstandard/freestanding entry points;
- [ ] `--platform` remains usable for provider selection;
- [ ] `--driver`/`--driver-arg` remain available so CForge can still serve as an alternate/reference driver;
- [ ] build-system documentation matches the actual driver protocol and artifact behavior;
- [ ] CKV and collections bootstrap manifests are end-to-end build/run acceptance projects.

## C14e — default-toolchain cutover

- [ ] the Rust compiler is the primary conformance execution path;
- [ ] all Forge-repository examples that are valid v1 compile with `forgec`;
- [ ] all currently applicable CForge examples execute through `forgec` with equivalent observable behavior;
- [ ] current Cosmic hosted semantic tests execute through the production Forge compiler;
- [ ] normal Forge/Cosmic CI no longer requires Java/Clojure/CForge to run ordinary Forge code;
- [ ] retain CForge in a bounded differential/reference lane;
- [ ] keep CI fan-out bounded: grouped suites, not one workflow per example.

## Tests

The C14 gate includes, at minimum:

1. formatting, workspace check/tests and Clippy;
2. complete parser/syntax-negative/semantic-negative conformance;
3. spec-example parse/check/run suite;
4. native executable conformance for every runtime language family;
5. hosted provider integration including process args, time, filesystem and locking;
6. Game of Life expected-output comparison;
7. collections bootstrap native smoke;
8. CKV set/get end-to-end test through `forge` and `forgec`;
9. build-system tests for all four target kinds, local dependencies, `:std`, `:entry`, expected output and program arguments;
10. CForge differential lane for shared cases;
11. current Cosmic native semantic suite.

## Risks and decisions

- The main v1 spec contains older examples that conflict with the later normative syntax clarification. C14 follows the clarification, then removes the contradiction from the main spec before declaring language compatibility complete.
- Parse acceptance is not language compatibility. A construct is complete only when its required static semantics and runtime semantics are covered at the appropriate layer.
- CForge behavior is an oracle only where it agrees with the normative v1 documents; CForge does not override the specification.
- Platform-specific host effects remain providers. Portable Forge libraries must not acquire hidden libc/OS dependencies.
- Lighting/SIA code generation is deliberately outside C14. C14 first establishes a complete hosted Forge v1 implementation and default toolchain.

## Completion gate

C14 is complete only when the production Rust Forge toolchain can be used as the normal implementation for the current language and package ecosystem: the authoritative v1 examples are accepted with their documented semantics, all executable v1 feature families run natively, all existing applicable Forge/CForge examples run through `forgec`, the current `forge.fdn` build-system contract works end to end, and the current hosted Cosmic semantic corpus no longer depends on CForge for normal execution. CForge remains as an independent differential reference implementation.
