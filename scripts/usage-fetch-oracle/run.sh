#!/usr/bin/env bash
# Executes supplemental tests against an immutable, unchanged Go source tree.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
source "$repo_root/scripts/run-external-env.sh"
external_env
oracle_commit=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
output=${1:?usage: run.sh OUTPUT_JSON}
output=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$output")
mkdir -p "$(dirname "$output")"
scratch=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-usage-620.XXXXXX")
source_root="$scratch/source"
cleanup() {
  git -C "$repo_root" worktree remove --force "$source_root" >/dev/null 2>&1 || true
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
git -C "$repo_root" worktree add --quiet --detach "$source_root" "$oracle_commit"
cp "$repo_root/scripts/usage-fetch-oracle/usage_test.go.txt" "$source_root/internal/usage/oracle_620_test.go"
cp "$repo_root/scripts/usage-fetch-oracle/cli_test.go.txt" "$source_root/cmd/symbrain/oracle_620_test.go"
oracle="$scratch/go.json"
native="$scratch/native.json"
if command -v cygpath >/dev/null 2>&1; then
  oracle_env=$(cygpath -w "$oracle")
  native_env=$(cygpath -w "$native")
else
  oracle_env=$oracle
  native_env=$native
fi
export USAGE_FETCH_ORACLE_620="$oracle_env"
export USAGE_FETCH_NATIVE_620="$native_env"
export COMPUTERNAME=intentionally-wrong-oracle-620-host
export GOTOOLCHAIN=go1.26.7
export CGO_ENABLED=0
go -C "$source_root" test ./internal/usage -run '^TestUsageFetchOracle620$' -count=1
go -C "$source_root" test ./cmd/symbrain -run '^TestUsageFetchCLIOracle620$' -count=1
git -C "$source_root" diff --exit-code --quiet
if [[ ${USAGE_FETCH_CAPTURE_ONLY:-0} == 1 ]]; then
  cp "$oracle" "$output"
  exit 0
fi
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-"$repo_root/target"}
cd "$repo_root"
cargo test --locked -p symbrain-usage usage_fetch_620 -- --nocapture
cargo test --locked -p symbrain-cli --lib usage_fetch_620 -- --nocapture
python3 "$repo_root/scripts/usage-fetch-oracle/controls.py" "$oracle" "$scratch/controls.json"
python3 "$repo_root/scripts/usage-fetch-oracle/receipt.py" "$oracle" "$native" "$output" "$oracle_commit" "$scratch/controls.json"
