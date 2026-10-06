#!/usr/bin/env bash
set -euo pipefail
umask 022
report_dir="${1:?usage: scripts/doctor-stdout-oracle/run.sh REPORT_DIRECTORY}"
mkdir -p "$report_dir"
owned="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/doctor-stdout-765.XXXXXX")"
trap 'rm -rf "$owned"' EXIT
# Every probe executes from an owned PROJECT, not from this build directory.
owned="$(cd "$owned" && pwd -P)"
doctor_target_dir="$(python3 - <<'PY'
from pathlib import Path
import os
print(Path(os.environ.get("CARGO_TARGET_DIR") or "target").resolve())
PY
)"
export CARGO_TARGET_DIR="$doctor_target_dir"
export DOCTOR_STDOUT_GO="$owned/symbrain-go"
export DOCTOR_STDOUT_RUST="$CARGO_TARGET_DIR/debug/symbrain"
export DOCTOR_STDOUT_EMBEDDED="$CARGO_TARGET_DIR/debug/examples/doctor_embedded_output"
./scripts/run-go-oracle.sh dcddcef0df5789123c7c9a7ebe6e01f10e941f2c build -o "$DOCTOR_STDOUT_GO" ./cmd/symbrain
cargo build --locked -p symbrain-cli --bin symbrain --example doctor_embedded_output
# Real /dev/full and bounded F_SETPIPE_SZ pipes have an explicit Linux owner.
for probe in process fix embedded controls; do
  export DOCTOR_STDOUT_REPORT="$report_dir/$probe.json"
  python3 "scripts/doctor-stdout-oracle/$probe.py" > "$report_dir/$probe.log" 2>&1
done
