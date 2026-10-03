#!/usr/bin/env bash
set -euo pipefail

output=${1:?usage: run.sh output-directory}
expected=$(python3 scripts/rust-coverage/report.py --print-tool-version)
if [[ $(cargo llvm-cov --version) != "cargo-llvm-cov $expected" ]]; then
  echo "Install cargo-llvm-cov $expected with --locked before measuring coverage" >&2
  exit 2
fi
mkdir -p "$output"
# Old results cannot stand in for a failed or incomplete measurement.
rm -f "$output/rust.json" "$output/summary.json"
floor=$(python3 scripts/rust-coverage/report.py --print-floor)
status=0
cargo llvm-cov --workspace --all-features --locked --json \
  --fail-under-lines "$floor" --output-path "$output/rust.json" || status=$?
if [[ -f "$output/rust.json" ]]; then
  python3 scripts/rust-coverage/report.py --report "$output/rust.json" \
    --summary "$output/summary.json" || status=$?
else
  echo "Coverage did not produce a complete LLVM report" >&2
  if [[ $status == 0 ]]; then status=1; fi
fi
exit "$status"
