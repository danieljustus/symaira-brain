#!/usr/bin/env bash
# Regenerates rust/symbrain-usage/tests/fixtures/usage_native_gates.json from the
# Go implementation at ORACLE_REV (default: this checkout's HEAD) without
# modifying tracked source: the supplemental test runs in a disposable archive.
set -euo pipefail
repo_root=$(git rev-parse --show-toplevel)
rev=${ORACLE_REV:-HEAD}
output=${1:-"$repo_root/rust/symbrain-usage/tests/fixtures/usage_native_gates.json"}
output=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$output")
scratch=${USAGE_768_SCRATCH:?set USAGE_768_SCRATCH to an empty directory on the external volume}
rm -rf "$scratch/source" "$scratch/run"
mkdir -p "$scratch/source" "$scratch/run"
git -C "$repo_root" archive "$rev" | tar -x -C "$scratch/source"
cp "$repo_root/scripts/usage-native-gates-oracle/gates_test.go.txt" "$scratch/source/internal/usage/gates_768_test.go"
cd "$scratch/source"
USAGE_768_GATES_OUT="$output" USAGE_768_GATES_ROOT="$scratch/run" CGO_ENABLED=0 \
  /Users/daniel/.local/bin/dev-external go test ./internal/usage -run '^TestNativeGatesOracle768$' -count=1
echo "fixture: $output (Go revision $(git -C "$repo_root" rev-parse "$rev"))"
