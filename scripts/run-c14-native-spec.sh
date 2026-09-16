#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FORGEC="${FORGEC:-$ROOT/target/debug/forgec}"

if [[ ! -x "$FORGEC" ]]; then
  echo "forgec not found at $FORGEC" >&2
  exit 1
fi

common=(--library "core=$ROOT/lib/core.fg")

for source in "$ROOT"/examples/conformance/run/*.fg; do
  echo "native-spec: $(basename "$source")"
  "$FORGEC" "${common[@]}" --run "$source"
done

echo "native-spec: local captured closures"
"$FORGEC" "${common[@]}" --run "$ROOT/examples/c14-native-spec/closures.fg"

for source in "$ROOT"/examples/c14-native-spec/run/*.fg; do
  echo "native-spec: $(basename "$source")"
  "$FORGEC" "${common[@]}" --run "$source"
done

for source in \
  "$ROOT/examples/c14-native-spec/trap_checked_add.fg" \
  "$ROOT/examples/c14-native-spec/trap_div_zero.fg"; do
  echo "native-spec expected trap: $(basename "$source")"
  if "$FORGEC" "${common[@]}" --run "$source" >/dev/null 2>&1; then
    echo "expected trap unexpectedly succeeded: $source" >&2
    exit 1
  fi
done

echo "native-spec: Game of Life"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
"$FORGEC" "${common[@]}" \
  --library "std.console=$ROOT/lib/std/console.fg" \
  --run "$ROOT/examples/game_of_life.fg" >"$tmp"
diff -u "$ROOT/examples/game_of_life.expected.txt" "$tmp"

echo "C14 native executable specification: PASS"
