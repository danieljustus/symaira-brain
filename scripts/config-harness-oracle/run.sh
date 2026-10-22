#!/usr/bin/env bash
set -euo pipefail
# The candidate base (CoreKit v0.18.2), as for the setup-repair, setup-source
# and doctor-repair oracles.
oracle_ref=0541e64a36d83bac8e5e4f8c4136033c03db83ae
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/config-harness-769.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
go_binary="$work_dir/symbrain-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symbrain"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "${CARGO_TARGET_DIR:-target}/debug/symbrain.exe")"
fi
report="${1:?usage: scripts/config-harness-oracle/run.sh OUTPUT_JSON [--record-fixture PATH]}"
shift
./scripts/run-go-oracle.sh "$oracle_ref" build -o "$go_binary" ./cmd/symbrain
cargo build -p symbrain-cli --bin symbrain --locked
rm -f "$report"
python3 scripts/config-harness-oracle/replay.py "$go_binary" "$rust_binary" "$report" "$@"
# A replay that silently ran another suite exits 0 without writing a report.
test -s "$report" || { echo "config-harness oracle wrote no report: $report" >&2; exit 1; }
