# Running Forge firmware on Lighting

This is the pre-Cosmic bring-up path for Forge on SIA32.

The immediate goal is deliberately smaller than booting an operating system:

```text
Forge root plus explicit semantic libraries
        |
        v
typed FIR
        |
        v
production Cranelift SIA32 backend
        |
        v
relocation-free SIA32 machine code
        |
        v
Lighting reset-ROM wrapper
        |
        v
LightingSimulation firmware.bin
        |
        v
instruction/MMIO/trap trace + diagnostic console
```

This lets compiler and simulator work meet continuously while SIA32 support is still being completed. Once this path can run progressively larger freestanding programs, the same compiler/backend work can be used for the eventual Cosmic image path.

## Current scope

`forge-lighting-firmware` is the current bootstrap SIA32 image producer. The
stable Forge-to-Cosmic boundary is recorded in
[`cosmic-compile-contract.md`](cosmic-compile-contract.md).

It currently:

- parses and type-checks a root Forge source plus explicit `--library NAME=PATH`
  semantic modules;
- lowers it through the normal HIR -> typed HIR -> FIR pipeline;
- selects the production `sia32-unknown-none` Cranelift backend;
- compiles the selected entry and reachable module functions to SIA32 machine
  code;
- resolves function-call relocations through the production SIAO32 linker;
- emits headerless raw and user images at an explicit architectural address;
- embeds the bytes behind a tiny Lighting reset-ROM assembly wrapper;
- calls the Forge entry from reset and halts Lighting when it returns.

The firmware entry must currently be:

```text
fn main() -> i32
```

(or another zero-argument `i32` function selected with `--entry`).

Function calls and explicit libraries use the real semantic module and SIAO32
linkers. Unsupported data/global relocation forms remain target boundaries and
must be added to that production path rather than through simulator-only
shortcuts.

## Prerequisites

You need checkouts of both repositories next to each other:

```text
work/
  forge/
  LightingSimulation/
```

or point the runner at another Lighting checkout with `LIGHTING_SIM`.

Build Forge:

```sh
cd forge
cargo build -p forge-compiler --bin forge-lighting-firmware --locked
```

The Lighting checkout supplies the authoritative SIA assembler and `lighting-run` machine runner.

## Run the example

From the Forge repository:

```sh
bash scripts/run-forge-lighting.sh
```

The example is:

```text
examples/lighting/firmware_hello.fg
```

It writes directly to Lighting's architectural diagnostic console at `0xfff03000`. The generated reset wrapper writes `1` to `HALT_CONTROL` after the Forge function returns.

Expected guest console output:

```text
FORGE ON LIGHTING
```

Artifacts are written to:

```text
build/lighting-firmware/firmware.s
build/lighting-firmware/firmware.bin
build/lighting-firmware/firmware.symbols
build/lighting-firmware/console.log
build/lighting-firmware/debug.log
```

`firmware.bin` is the actual Lighting ROM firmware blob.

## Use a different Lighting checkout

```sh
LIGHTING_SIM=/path/to/LightingSimulation \
  bash scripts/run-forge-lighting.sh
```

This is useful while the full hardware-composition simulator is evolving on a branch. The Forge repository does not vendor or modify LightingSimulation.

## Compile without running

Generate the Lighting assembly wrapper:

```sh
cargo run -p forge-compiler --bin forge-lighting-firmware -- \
  examples/lighting/firmware_hello.fg \
  -o build/firmware.s
```

Then run it:

```sh
cargo run --manifest-path ../LightingSimulation/Cargo.toml \
  --bin lighting-run -- \
  build/firmware.bin \
  --symbols build/firmware.symbols \
  --console stdout \
  --trace-instructions \
  --trace-mmio \
  --trace-traps \
  --trace-interrupts \
  --history 256
```

## Debugging

The runner script separates architectural output from simulator diagnostics:

- `console.log` contains bytes written by the guest to the diagnostic console.
- `debug.log` contains the simulator's instruction/MMIO/trap/interrupt trace and failure dump.
- `firmware.symbols` lets `lighting-run` display wrapper/entry symbols instead of only raw addresses.

For MMU work, add `--trace-mmu` to the `lighting-run` command.

To stop at the generated Forge entry:

```sh
cargo run --manifest-path ../LightingSimulation/Cargo.toml \
  --bin lighting-run -- \
  build/lighting-firmware/firmware.bin \
  --symbols build/lighting-firmware/firmware.symbols \
  --break 0xffff0000 \
  --trace-instructions
```

Use the actual `forge_entry` address from `firmware.symbols` when you want the breakpoint after the reset shim rather than at reset itself.

Physical memory/MMIO watchpoints are also available:

```text
--watch-read ADDRESS
--watch-write ADDRESS
--watch-rw ADDRESS
```

## Why the reset wrapper exists

A normal Forge function follows the SIA function ABI and ends with a return. Firmware reset code has no caller, so the generated wrapper provides one:

```text
RESET_VECTOR
    |
    +-- BL forge_entry
    |       |
    |       +-- Forge-generated SIA32
    |       +-- RET
    |
    +-- HALT_CONTROL = 1
```

This keeps the compiler-generated function ordinary and testable while giving Lighting a complete firmware lifecycle.

## Roadmap to Cosmic

This bring-up path should grow in small, testable steps:

1. one relocation-free Forge function on Lighting;
2. MMIO and diagnostic-console firmware;
3. function calls and linked SIAO32 text;
4. rodata/data/bss and global relocations;
5. freestanding Forge libraries;
6. interrupt/trap and PLIO firmware;
7. larger standalone firmware diagnostics;
8. Cosmic kernel/userspace image production;
9. boot Cosmic through the full Lighting hardware composition.

The rule is that each step must execute on Lighting with observable behavior. Producing a SIA object or image without executing it is not sufficient acceptance evidence.
