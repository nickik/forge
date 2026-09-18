# Forge

Forge is a statically typed, ahead-of-time systems language for systems software. It combines C-like syntax and flat value layouts with immutable-by-default bindings, explicit allocation, tagged unions, pattern matching, checked operations, and a compiler pipeline designed to target both hosted machines and DEC's SIA32 systems.

This repository contains the Forge v1 specification, bootstrap compiler, build system, conformance suites, standard-library work, and native code-generation work.

## Current status

Forge is under active compiler development. The current production compiler path is:

```text
Forge source
  -> lexer / parser
  -> HIR
  -> typed HIR
  -> FIR
  -> Cranelift
  -> native object / executable
```

The mature hosted target is currently **AArch64 Linux**. There is also an **RV64 execution lane**, primarily used to validate cross-target code generation under QEMU. **x86/x86-64 is not a supported Forge target**, so an x86-64 development machine cannot run Forge-produced AArch64 binaries directly.

SIA32 is the target for Lighting/Cosmic work and is still being completed. Native SIA32 acceptance ultimately means producing a loadable SIA image and executing it through LightingSimulation; object emission by itself is not considered sufficient.

## Local setup

### 1. Install Rust

Install a current stable Rust toolchain with `rustup` and verify:

```sh
rustc --version
cargo --version
```

Clone the repository and build the toolchain:

```sh
git clone https://github.com/nickik/forge.git
cd forge

cargo build --workspace --locked
cargo test --workspace --locked
```

The two user-facing bootstrap commands are:

- `forgec` — compile/check/run an individual Forge source unit.
- `forge` — package/build-system driver using `forge.fdn`.

For convenient local use:

```sh
cargo build -p forge-compiler -p forge-build --bins --locked
export PATH="$PWD/target/debug:$PATH"

forgec --help
forge graph --manifest-path examples/c14-build-system/forge.fdn
```

### 2. Running on an AArch64 Linux host

On AArch64 Linux, the normal hosted compiler can build and run examples directly.

Run the native language acceptance suite:

```sh
scripts/run-c14-native-spec.sh
```

Run a single source file:

```sh
forgec \
  --library core="$PWD/lib/core.fg" \
  --run examples/conformance/run/01-arithmetic-precedence.fg
```

Run Game of Life:

```sh
forgec \
  --library core="$PWD/lib/core.fg" \
  --library std.console="$PWD/lib/std/console.fg" \
  --run examples/game_of_life.fg
```

The expected output is checked in as `examples/game_of_life.expected.txt`.

### 3. Running from x86-64

Forge does **not** currently emit x86-64 code. The default hosted target emitted by `forgec` is AArch64 Linux, so use an AArch64 Linux environment under QEMU (or an AArch64 VM) to build and execute the hosted examples.

On Debian/Ubuntu, install QEMU:

```sh
sudo apt-get update
sudo apt-get install qemu-system-arm qemu-user-static
```

The simplest reliable setup is an **AArch64 Linux VM under `qemu-system-aarch64`**. Boot an AArch64 Linux image, clone this repository inside the guest, install Rust there, then use the AArch64 commands above. This keeps the compiler, system linker, libc, and generated executables in one matching architecture environment.

QEMU user-mode can also execute suitable AArch64 Linux binaries when the matching AArch64 userspace/sysroot is available, but Forge's current `--run` path builds and launches the executable itself. For day-to-day development on x86-64, a full AArch64 guest is therefore less fragile than trying to make every host linker/runtime lookup cross-architecture aware.

### 4. RV64/QEMU backend validation

The repository also contains RV64 code-generation tests. CI uses QEMU user mode plus the RISC-V GNU binutils:

```sh
sudo apt-get update
sudo apt-get install qemu-user binutils-riscv64-linux-gnu

qemu-riscv64 --version
riscv64-linux-gnu-as --version

FORGE_RISCV64_EXECUTION=1 \
  cargo test --workspace --exclude forge-frontend --locked -- --nocapture
```

This is a compiler/backend validation lane. It does not mean the `forgec` CLI currently accepts RV64 as its normal C14 hosted target.

## Build-system example

`examples/c14-build-system` demonstrates the package driver, executable targets, tests, local libraries, and a freestanding kernel target.

```sh
export PATH="$PWD/target/debug:$PATH"

forge graph --manifest-path examples/c14-build-system/forge.fdn

forge check \
  --manifest-path examples/c14-build-system/forge.fdn

forge build \
  --manifest-path examples/c14-build-system/forge.fdn \
  --target app

forge run \
  --manifest-path examples/c14-build-system/forge.fdn \
  --target app \
  -- alpha beta

forge test \
  --manifest-path examples/c14-build-system/forge.fdn \
  --target smoke

forge build \
  --manifest-path examples/c14-build-system/forge.fdn \
  --target kernel
```

The kernel build produces a freestanding object rather than a hosted program. See `docs/build-system.md` for `forge.fdn`, local dependencies, target kinds, entry symbols, and the compiler-driver protocol.

## Useful examples

| Example | Purpose |
|---|---|
| `examples/parser-demo.fg` | Small parser/AST demonstration |
| `examples/conformance/run/` | Core executable language semantics |
| `examples/c14-native-spec/run/` | C14 native feature/ABI coverage |
| `examples/game_of_life.fg` | Larger hosted Forge program |
| `examples/c14-build-system/` | Package/build/run/test/kernel workflow |
| `examples/ckv/` | Multi-package example |
| `examples/collections-bootstrap-smoke/` | Native collections bootstrap smoke test |

Parser-only inspection remains useful while developing the frontend:

```sh
cargo run -p forge-frontend --bin forge-parse -- \
  --json examples/parser-demo.fg
```

## Developer validation

Before submitting compiler changes, run:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For the same native acceptance exercised by CI on AArch64:

```sh
cargo build -p forge-build -p forge-compiler --bins --locked
export PATH="$PWD/target/debug:$PATH"
scripts/run-c14-native-spec.sh
```

On a machine with the RV64 tools installed, also enable the QEMU execution lane shown above.

## Repository map

- `crates/forge-frontend/` — lexer, parser, AST, HIR/typechecking frontend work.
- `crates/forge-fir/` — Forge Intermediate Representation and verification.
- `crates/forge-codegen-cranelift/` — Cranelift lowering and native object generation.
- `crates/forge-compiler/` — `forgec`, linking, hosted runtime and compiler orchestration.
- `crates/forge-build/` — `forge` package/build-system command.
- `crates/forge-conformance/` — conformance-suite runner.
- `docs/forge-v1-spec.md` — normative Forge v1 language specification.
- `docs/frontend-ir.md` — AST/HIR/FIR representation decisions.
- `docs/compiler-architecture.md` — compiler architecture.
- `docs/build-system.md` — package/build system.
- `COMPILER_EVOLUTION_TODO.md` — current compiler roadmap.
- `C15_TODO.md` — SIA32 milestone prerequisites and acceptance work.

## Compiler representation rule

Do not generate backend IR directly from parser nodes. Forge keeps semantics explicit through:

1. **AST** — source-faithful syntax and spans.
2. **HIR** — resolved names and desugared source constructs.
3. **Typed HIR** — exact types and semantic decisions.
4. **FIR** — typed CFG/three-address representation with explicit checks and cleanup.
5. **Backend IR** — currently primarily Cranelift for native targets.

## Design rule

When implementation and specification disagree, **the specification wins** unless a language change is explicitly recorded and the normative specification is updated in the same change.
