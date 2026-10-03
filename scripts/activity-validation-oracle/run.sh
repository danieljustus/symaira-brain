#!/usr/bin/env bash
set -euo pipefail
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/activity-validation-767.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
report="${1:?usage: run.sh OUTPUT_JSON}"
go_binary="$work_dir/symbrain-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symbrain"
fixture="$work_dir/activity-cli.json"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "${CARGO_TARGET_DIR:-target}/debug/symbrain.exe")"
  fixture="$(cygpath -m "$fixture")"
fi
./scripts/run-go-oracle.sh "$oracle_ref" build -ldflags '-X main.version=dev' -o "$go_binary" ./cmd/symbrain
./scripts/run-go-oracle.sh "$oracle_ref" run ./scripts/activity-cli-oracle -go-binary "$go_binary" -output "$fixture"
cargo build -p symbrain-cli --bin symbrain --locked
SYMBRAIN_ACTIVITY_CLI_ORACLE_FIXTURE="$fixture" cargo test -p symbrain-cli --test activity_cli_oracle_tests --locked
python3 scripts/activity-validation-oracle/replay.py "$go_binary" "$rust_binary" "$report"
SYMBRAIN_ACTIVITY_VALIDATION_FIXTURE="$report.fixture.json" cargo test -p symbrain-cli --test activity_validation_tests --locked
python3 scripts/activity-validation-oracle/controls.py "$report.controls.json"
