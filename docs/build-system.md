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

A kernel target defaults to `:std false`; other targets default to hosted `std`. A target may additionally specify `:entry "symbol"`. Linker metadata is reserved for the next implementation step and does not yet affect driver invocation.

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

The bootstrap compiler-driver protocol is deliberately small:

```text
<driver> [prefix args...] --check <target-root>
<driver> [prefix args...] --run   <target-root>
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

This separation is intentional. Manifest/dependency semantics belong to Forge; compiler and interpreter implementations conform to the driver protocol.

## OS direction

The manifest already reserves the target model needed by Cosmic: `:kernel`, `:std false`, custom entry symbols, and local library packages. The next build-system milestones are linker configuration, target triples/platform-provider selection, workspaces, generated build configuration, QEMU runners, and a lock file once dependency sources extend beyond local paths.
