#!/usr/bin/env bash
# Isolated credential reference constructor/request oracle with no live endpoints.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
source "$repo_root/scripts/run-external-env.sh"
external_env
output=${1:?usage: run.sh OUTPUT_JSON}
output=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$output")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-usage-768.XXXXXX")
source_root="$scratch/source"
evidence_dir="${output%.json}.evidence"
mkdir -p "$evidence_dir"
cleanup() {
  stage_exit=$?
  for receipt in go native cli controls; do
    if [[ -f "$scratch/$receipt.json" ]]; then cp "$scratch/$receipt.json" "$evidence_dir/$receipt.json"; fi
  done
  if [[ $stage_exit != 0 ]]; then
    python3 - "$output" "$stage_exit" <<'PYFAIL'
import json, pathlib, subprocess, sys
pathlib.Path(sys.argv[1]).write_text(json.dumps({"schema_version":1,"gate_exit":int(sys.argv[2]),"candidate_head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"accepted":False},indent=2)+"\n")
PYFAIL
  fi
  git -C "$repo_root" worktree remove --force "$source_root" >/dev/null 2>&1 || true
  rm -rf "$scratch"
}
run_stage() {
  local label=$1
  shift
  if "$@" >"$evidence_dir/$label.log" 2>&1; then
    tail -n 12 "$evidence_dir/$label.log"
  else
    local stage_exit=$?
    tail -n 100 "$evidence_dir/$label.log" >&2
    return "$stage_exit"
  fi
}
trap cleanup EXIT INT TERM
oracle_commit=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
git -C "$repo_root" worktree add --quiet --detach "$source_root" "$oracle_commit"
cp "$repo_root/scripts/usage-credential-oracle/provider_test.go.txt" "$source_root/internal/usage/oracle_768_test.go"
mkdir -p "$scratch/bin" "$scratch/home"
rustc --edition=2024 "$repo_root/rust/symbrain-usage/tests/support/fake_symvault.rs" -o "$scratch/bin/symvault$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )"
cp "$scratch/bin/symvault"* "$scratch/bin/security$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )"
export USAGE_768_ORACLE="$scratch/go.json" USAGE_768_NATIVE="$scratch/native.json" USAGE_768_ROOT="$scratch/home" USAGE_768_BIN="$scratch/bin"
if command -v cygpath >/dev/null 2>&1; then
  export USAGE_768_ORACLE=$(cygpath -w "$USAGE_768_ORACLE") USAGE_768_NATIVE=$(cygpath -w "$USAGE_768_NATIVE") USAGE_768_ROOT=$(cygpath -w "$USAGE_768_ROOT") USAGE_768_BIN=$(cygpath -w "$USAGE_768_BIN")
fi
export GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0
run_stage go-build go -C "$source_root" build -trimpath -o "$scratch/go-usage" ./cmd/symbrain
run_stage go-constructors go -C "$source_root" test ./internal/usage -run '^TestUsageCredentialOracle768$' -count=1
git -C "$source_root" diff --exit-code --quiet
cd "$repo_root"
run_stage native-constructors cargo test --locked -p symbrain-usage --test credential_reference_tests -- --ignored --nocapture
mkdir -p "$(dirname "$output")"
run_stage native-build cargo build --locked -p symbrain-cli
run_stage cli python3 "$repo_root/scripts/usage-credential-oracle/cli.py" "$scratch/go-usage" "${CARGO_TARGET_DIR:-target}/debug/symbrain$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )" "$scratch/bin/symvault$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )" "$scratch/cli.json"
run_stage controls python3 "$repo_root/scripts/usage-credential-oracle/controls.py" "$scratch" "${CARGO_TARGET_DIR:-target}/debug/symbrain$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )"
python3 "$repo_root/scripts/usage-credential-oracle/receipt.py" "$scratch/go.json" "$scratch/native.json" "$output" "$oracle_commit"
