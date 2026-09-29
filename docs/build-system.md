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

`forge graph` prints the dependency-first package order and never launches a
driver. The other commands select every root-package target when `--target` is
omitted; `--target NAME` selects exactly that target.

Common options:

```text
--manifest-path PATH
--target NAME
--platform NAME
--driver PROGRAM
--driver-arg ARG
```

`FORGE_DRIVER` may supply the driver executable when `--driver` is omitted.
Each repeated `--driver-arg` is passed to that executable in command-line
order, before Forge's library, platform, action and target arguments.
When `--platform` is present, Forge forwards it unchanged to the selected
driver before the action. The production `forgec` driver currently accepts
only `host`; other provider identities remain reserved until their production
implementations exist.

Forge constructs each compiler-driver command in this order:

1. repeated `--driver-arg` values;
2. shipped `core` and dependency `--library NAME=ROOT` inputs;
3. optional `--platform NAME` and, for `forge run`, repeated
   `--program-arg ARG` values;
4. shipped hosted `std` library inputs when the target has `:std true`;
5. optional `--entry NAME` for non-check actions;
6. the action, target root and any output path.

Using `<driver-options>` for steps 2 through 4, the action-specific protocol
is:

```text
<driver> [prefix args...] <driver-options> --check <target-root>
<driver> [prefix args...] <driver-options> [--entry NAME] --build <target-root> -o <artifact>
<driver> [prefix args...] <driver-options> [--entry NAME] --emit-object <target-root> -o <artifact>
<driver> [prefix args...] <driver-options> [--entry NAME] --run <target-root>
```

`forge check` never passes `--entry` and does not request an artifact. `forge
build` emits hosted executable/test targets under `build/<target>` and
freestanding kernel objects under `build/<target>.o`. A successful driver exit
without the requested artifact is a build failure. Library targets remain
source compilation units during the bootstrap, so building a library performs
the same `--check` semantic gate and creates no artifact; serialized
compiled-library interfaces remain a later package-system milestone.

`forge run -- ARGS...` selects executable/test targets, forwards each argument
as `--program-arg ARG`, invokes the driver's `--run` action and relays its
stdout/stderr. Program arguments are rejected for other actions. Library and
kernel targets are not executed.

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

CForge is maintained separately from this repository, so the concrete command
depends on that checkout. Any selected CForge executable must implement the
same protocol above. This separation is intentional: manifest/dependency
semantics belong to Forge, while alternate compilers and interpreters conform
to its driver protocol. The default path remains the production `forgec`
driver.

## OS direction

The manifest already reserves the target model needed by Cosmic: `:kernel`, `:std false`, custom entry symbols, and local library packages. The next build-system milestones are general compiled-library interfaces, linker configuration, target triples/platform-provider selection, workspaces, generated build configuration, QEMU runners, and a lock file once dependency sources extend beyond local paths.
