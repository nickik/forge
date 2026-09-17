# Forge build system

Forge's build system is part of the Forge toolchain and lives in this repository. The bootstrap implementation is the `forge-build` Rust crate and exposes the `forge` command.

The first implementation intentionally supports only local projects and local path dependencies. Registry, Git, publishing, arbitrary build scripts, feature resolution and version solving are deferred.

## Manifest

A package is described by `forge.fdn`:

```fdn
#forge/package {
  :name "game-of-life"
  :version "0.1.0"

  :targets {
    :main {
      :kind :executable
      :root "examples/game_of_life.fg"
      :test {:expected "examples/game_of_life.expected.txt"}
    }
  }

  :dependencies {}
}
```

Supported target kinds are `:library`, `:executable`, `:kernel`, and `:test`.

A kernel target requires `:std false`; explicitly enabling hosted `std` on a
kernel is rejected while other target kinds default to hosted `std`. A target
may additionally specify `:entry "symbol"`. Forge passes that entry to the
compiler for non-check actions. Native object emission selects the named Forge
function and exports it under that exact platform-facing symbol; other
functions retain deterministic internal Forge symbols. General linker metadata
is reserved for a later implementation step.

## Local path dependencies

A dependency is currently only:

```fdn
:dependencies {
  :cosmic-abi {:path "../abi"}
}
```

The path names a directory containing another `forge.fdn`. The dependency key must equal that package's `:name`.

The resolver:

- canonicalizes manifests;
- recursively loads local path dependencies;
- rejects cycles;
- rejects one package name resolving to multiple local paths;
- validates target roots;
- produces deterministic dependency-first order.

For the bootstrap driver protocol, each dependency package must currently expose exactly one `:library` target. Forge passes its root source file to the compiler/interpreter before the root target:

```text
<driver> [prefix args...] \
  --library cosmic_abi=/absolute/path/to/abi/src/lib.fg \
  --check <target-root>
```

Package names containing `-` are mapped to Forge module identifiers using `_` for this bootstrap mapping. The supplied library source must declare the matching module name. The driver is responsible for parsing that unit, exposing only public declarations, resolving imports and compiling/linking the root target against it.

This is intentionally a bootstrap representation. A future compiler interface may consume compiled module interfaces rather than source-root paths, without changing `forge.fdn` dependency semantics.

## Commands

```text
forge graph
forge check
forge build
forge run
forge test
```

Common options:

```text
--manifest-path PATH
--target NAME
--driver PROGRAM
--driver-arg ARG
```

`FORGE_DRIVER` may supply the driver executable when `--driver` is omitted.

The bootstrap compiler-driver protocol is:

```text
<driver> [prefix args...] [--library NAME=ROOT]... --check <target-root>
<driver> [prefix args...] [--library NAME=ROOT]... --run   <target-root>
```

`forge build` currently performs the same semantic compilation gate as `forge check`, because the production compiler does not yet expose a final code-generation interface. This will split once native artifacts exist.

`forge test` runs selected targets. When a target contains:

```fdn
:test {:expected "path/to/output.txt"}
```

its stdout must match that file byte-for-byte. A target without `:expected` passes when the driver exits successfully.

## CForge bootstrap driver

The same build system can drive CForge without knowing anything about Clojure:

```text
forge test \
  --manifest-path forge.fdn \
  --driver clojure \
  --driver-arg -M:native \
  --driver-arg --
```

or the native CForge executable:

```text
forge test --driver ./target/cforge
```

CForge currently implements the library-source side of this protocol: it validates module identity, parses `pub fn` declarations, rejects private or missing symbols and checks call arity. During the bootstrap it can execute imported functions whose bodies are a single return expression by lowering them into the root semantic tree. General cross-unit calls are the next interpreter/compiler milestone; this limitation is an implementation detail, not a source-language or build-system restriction.

This separation is intentional. Manifest/dependency semantics belong to Forge; compiler and interpreter implementations conform to the driver protocol.

## OS direction

The manifest already reserves the target model needed by Cosmic: `:kernel`, `:std false`, custom entry symbols, and local library packages. The next build-system milestones are general compiled-library interfaces, linker configuration, target triples/platform-provider selection, workspaces, generated build configuration, QEMU runners, and a lock file once dependency sources extend beyond local paths.
