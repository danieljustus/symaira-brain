#!/usr/bin/env bash
# Execute real Hermes constructors/strategy requests against immutable Go.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
source "$repo_root/scripts/run-external-env.sh"
external_env
output=${1:?usage: run.sh OUTPUT_JSON}
output=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$output")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-hermes-768.XXXXXX")
source_root="$scratch/source"
evidence_dir="${output%.json}.evidence"
mkdir -p "$evidence_dir"
cleanup() {
  stage_exit=$?
  for receipt in go native input cli controls filesystem; do
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
trap cleanup EXIT INT TERM
run_stage() {
  local label=$1
  shift
  if "$@" >"$evidence_dir/$label.log" 2>&1; then tail -n 12 "$evidence_dir/$label.log"; else local code=$?; tail -n 100 "$evidence_dir/$label.log" >&2; return "$code"; fi
}
cd "$repo_root"
python3 scripts/usage-hermes-oracle/cases.py "$scratch/input.json"
mkdir -p "$scratch/home"
export USAGE_HERMES_ORACLE="$scratch/go.json" USAGE_HERMES_NATIVE="$scratch/native.json" USAGE_HERMES_INPUT="$scratch/input.json" USAGE_HERMES_ROOT="$scratch/home"
if command -v cygpath >/dev/null 2>&1; then
  export USAGE_HERMES_ORACLE=$(cygpath -w "$USAGE_HERMES_ORACLE") USAGE_HERMES_NATIVE=$(cygpath -w "$USAGE_HERMES_NATIVE") USAGE_HERMES_INPUT=$(cygpath -w "$USAGE_HERMES_INPUT") USAGE_HERMES_ROOT=$(cygpath -w "$USAGE_HERMES_ROOT")
fi
oracle_commit=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
git -C "$repo_root" worktree add --quiet --detach "$source_root" "$oracle_commit"
cp "$repo_root/scripts/usage-hermes-oracle/provider_test.go.txt" "$source_root/internal/usage/oracle_hermes_768_test.go"
export GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0
executable_suffix=$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )
run_stage go-build go -C "$source_root" build -trimpath -o "$scratch/go-usage$executable_suffix" ./cmd/symbrain
run_stage go-constructors go -C "$source_root" test ./internal/usage -run '^TestUsageHermesOracle768$' -count=1
git -C "$source_root" diff --exit-code --quiet
run_stage native-constructors cargo test --locked -p symbrain-usage --test hermes_credential_tests -- --ignored --nocapture
run_stage native-build cargo build --locked -p symbrain-cli
run_stage cli python3 "$repo_root/scripts/usage-hermes-oracle/cli.py" "$scratch/go.json" "$scratch/go-usage$executable_suffix" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix" "$scratch/cli.json"
run_stage filesystem python3 "$repo_root/scripts/usage-hermes-oracle/filesystem.py" "$scratch/go-usage$executable_suffix" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix" "$scratch/filesystem.json"
run_stage controls python3 "$repo_root/scripts/usage-hermes-oracle/controls.py" "$scratch" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix"
python3 "$repo_root/scripts/usage-hermes-oracle/receipt.py" "$scratch/go.json" "$scratch/native.json" "$output" "$oracle_commit"
