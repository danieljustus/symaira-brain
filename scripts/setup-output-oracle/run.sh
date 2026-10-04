#!/usr/bin/env bash
set -euo pipefail
umask 022
report_dir="${1:?usage: scripts/setup-output-oracle/run.sh REPORT_DIRECTORY}"
mkdir -p "$report_dir"
owned="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/setup-output-765.XXXXXX")"
trap 'rm -rf "$owned"' EXIT
export SETUP_OUTPUT_GO="$owned/symbrain-go"
export SETUP_OUTPUT_RUST="${CARGO_TARGET_DIR:-target}/debug/symbrain"
export SETUP_OUTPUT_EMBEDDED_RUST="${CARGO_TARGET_DIR:-target}/debug/examples/setup_embedded_output"
./scripts/run-go-oracle.sh dcddcef0df5789123c7c9a7ebe6e01f10e941f2c build -o "$SETUP_OUTPUT_GO" ./cmd/symbrain
cargo build --locked -p symbrain-cli --bin symbrain --example setup_embedded_output
# Actual /dev/full and F_SETPIPE_SZ inputs are Linux-specific. Their complete
# gate has an explicit Linux CI owner; no skipped run is three-OS evidence.
for probe in output-probes source-output pipe-probes human-pipe-probes embedded controls; do
  export SETUP_OUTPUT_REPORT="$report_dir/$probe.json"
  python3 "scripts/setup-output-oracle/$probe.py" > "$report_dir/$probe.log" 2>&1
done
