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
  "$ROOT/examples/c14-native-spec/trap_div_zero.fg" \
  "$ROOT/examples/c14-native-spec/trap_bitstruct_range.fg"; do
  echo "native-spec expected trap: $(basename "$source")"
  if "$FORGEC" "${common[@]}" --run "$source" >/dev/null 2>&1; then
    echo "expected trap unexpectedly succeeded: $source" >&2
    exit 1
  fi
done

echo "native-spec expected rejection: anonymous closure to function pointer"
closure_error="$(mktemp)"
if "$FORGEC" "${common[@]}" --run \
  "$ROOT/examples/c14-native-spec/reject_closure_fn_pointer.fg" \
  >/dev/null 2>"$closure_error"; then
  echo "anonymous closure unexpectedly coerced to a function pointer" >&2
  rm -f "$closure_error"
  exit 1
fi
if ! grep -q "capture-free anonymous function values" "$closure_error"; then
  echo "anonymous closure failed for the wrong reason:" >&2
  cat "$closure_error" >&2
  rm -f "$closure_error"
  exit 1
fi
rm -f "$closure_error"

echo "native-spec expected rejection: incompatible Result propagation"
result_error="$(mktemp)"
if "$FORGEC" "${common[@]}" --run \
  "$ROOT/examples/c14-native-spec/reject_result_try_error.fg" \
  >/dev/null 2>"$result_error"; then
  echo "incompatible Result propagation unexpectedly succeeded" >&2
  rm -f "$result_error"
  exit 1
fi
if ! grep -q "try/error-type" "$result_error"; then
  echo "incompatible Result propagation failed for the wrong reason:" >&2
  cat "$result_error" >&2
  rm -f "$result_error"
  exit 1
fi
rm -f "$result_error"

echo "native-spec: Game of Life"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT
"$FORGEC" "${common[@]}" \
  --library "std.console=$ROOT/lib/std/console.fg" \
  --run "$ROOT/examples/game_of_life.fg" >"$tmp"
diff -u "$ROOT/examples/game_of_life.expected.txt" "$tmp"

echo "C14 native executable specification: PASS"
