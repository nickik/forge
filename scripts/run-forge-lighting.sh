#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIGHTING_SIM="${LIGHTING_SIM:-$ROOT/../LightingSimulation}"
SOURCE="${1:-$ROOT/examples/lighting/firmware_hello.fg}"
OUT_DIR="${FORGE_LIGHTING_OUT:-$ROOT/build/lighting-firmware}"
MAX_STEPS="${FORGE_LIGHTING_MAX_STEPS:-100000}"

if [[ ! -f "$LIGHTING_SIM/Cargo.toml" ]]; then
  echo "LightingSimulation not found at $LIGHTING_SIM" >&2
  echo "Set LIGHTING_SIM=/path/to/LightingSimulation" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"
ASM="$OUT_DIR/firmware.s"
BIN="$OUT_DIR/firmware.bin"
SYMS="$OUT_DIR/firmware.symbols"
CONSOLE="$OUT_DIR/console.log"
TRACE="$OUT_DIR/debug.log"

cargo run --quiet --manifest-path "$ROOT/Cargo.toml" -p forge-compiler \
  --bin forge-lighting-firmware -- "$SOURCE" -o "$ASM"

cargo run --quiet --manifest-path "$LIGHTING_SIM/Cargo.toml" \
  --bin siaasm -- "$ASM" -o "$BIN" --symbols >"$SYMS"

set +e
cargo run --quiet --manifest-path "$LIGHTING_SIM/Cargo.toml" \
  --bin lighting-run -- "$BIN" \
  --symbols "$SYMS" \
  --console stdout \
  --console-log "$CONSOLE" \
  --max-steps "$MAX_STEPS" \
  --history 256 \
  --trace-instructions \
  --trace-mmio \
  --trace-traps \
  --trace-interrupts \
  2>"$TRACE"
status=$?
set -e

echo
echo "Firmware blob: $BIN"
echo "Generated wrapper: $ASM"
echo "Symbols: $SYMS"
echo "Console log: $CONSOLE"
echo "Debug trace: $TRACE"

if [[ $status -ne 0 ]]; then
  echo "Lighting exited with status $status; inspect $TRACE" >&2
  exit "$status"
fi
