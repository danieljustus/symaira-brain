#!/usr/bin/env bash
set -euo pipefail
# Same immutable revision as the sibling replay oracles; cmd/symbrain and
# internal/ are byte-identical to it at the candidate base.
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
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
python3 scripts/config-harness-oracle/replay.py "$go_binary" "$rust_binary" "$report" "$@"
