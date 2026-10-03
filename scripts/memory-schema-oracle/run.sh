#!/usr/bin/env bash
# Retain Go's real legacy false-positive as the intentional #649 correction.
set -euo pipefail
umask 022
report="${1:?usage: scripts/memory-schema-oracle/run.sh REPORT}"
owned="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/memory-schema-649.XXXXXX")"
trap 'rm -rf "$owned"' EXIT
git archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c | tar -xf - -C "$owned"
go_binary="$owned/symbrain-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symbrain"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "$rust_binary.exe")"
fi
(cd "$owned" && CGO_ENABLED=0 GOTOOLCHAIN=go1.26.7 go build -mod=readonly -o "$go_binary" ./cmd/symbrain)
cargo build --locked -p symbrain-cli --bin symbrain
python3 scripts/memory-schema-oracle/replay.py --go "$go_binary" --rust "$rust_binary" --report "$report"
if python3 scripts/memory-schema-oracle/replay.py --go "$go_binary" --rust "$go_binary" --report "$report.false-green-control.json" > "$report.false-green-control.log" 2>&1; then
  echo 'FAIL: unchanged Go false-green Doctor accepted as corrected native behavior' >&2
  exit 1
fi
python3 - "$report.false-green-control.log" <<'PY'
import pathlib,sys
text=pathlib.Path(sys.argv[1]).read_text()
assert 'AssertionError' in text and "'quick_check': 'ok'" in text, text
print('Actual unchanged Go false-green control rejected')
PY
