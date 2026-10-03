#!/usr/bin/env bash
set -euo pipefail
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/setup-repair-765.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
go_binary="$work_dir/symbrain-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symbrain"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "${CARGO_TARGET_DIR:-target}/debug/symbrain.exe")"
fi
report="${1:?usage: scripts/setup-repair-oracle/run.sh OUTPUT_JSON}"
./scripts/run-go-oracle.sh "$oracle_ref" build -o "$go_binary" ./cmd/symbrain
cargo build -p symbrain-cli --bin symbrain --locked
python3 scripts/setup-repair-oracle/replay.py "$go_binary" "$rust_binary" "$report"
