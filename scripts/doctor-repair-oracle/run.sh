#!/usr/bin/env bash
set -euo pipefail
umask 022
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/doctor-repair-765.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
go_binary="$work_dir/symbrain-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symbrain"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "${CARGO_TARGET_DIR:-target}/debug/symbrain.exe")"
fi
report="${1:?usage: scripts/doctor-repair-oracle/run.sh OUTPUT_JSON}"
./scripts/run-go-oracle.sh "$oracle_ref" build -o "$go_binary" ./cmd/symbrain
cargo build -p symbrain-cli --bin symbrain --locked
python3 scripts/doctor-repair-oracle/replay.py "$go_binary" "$rust_binary" "$report"
for control in wrong-exit missing-header missing-core-event; do
  control_report="$report.$control.json"
  if python3 scripts/doctor-repair-oracle/replay.py "$go_binary" "$rust_binary" "$control_report" "$control"; then
    echo "Doctor repair control was incorrectly accepted: $control" >&2
    exit 1
  fi
  python3 - "$control_report" "$control" <<'PY'
import json, sys
data = json.load(open(sys.argv[1]))
expected = {"wrong-exit":"exit", "missing-header":"stdout_base64", "missing-core-event":"logs"}[sys.argv[2]]
assert data["control"] == sys.argv[2] and data["total"] == data["complete_observations"] == 1
assert data["matched"] == 0 and data["exit"] == 1
assert data["failures"] == [{"case":"correct","fields":[expected]}], data["failures"]
PY
done
