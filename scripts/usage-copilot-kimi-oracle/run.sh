#!/usr/bin/env bash
# Execute real Copilot/Kimi file constructors/strategy requests against immutable Go.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
source "$repo_root/scripts/run-external-env.sh"
external_env
output=${1:?usage: run.sh OUTPUT_JSON}
output=$(python3 -c 'import os,sys; print(os.path.abspath(sys.argv[1]))' "$output")
temporary_root=${TMPDIR:-/tmp}
if command -v cygpath >/dev/null 2>&1; then
  temporary_root=$(cygpath -u "$temporary_root")
fi
scratch=$(mktemp -d "$temporary_root/symbrain-copilot-kimi-768.XXXXXX")
source_root="$scratch/source"
parent_root="$scratch/argv-parent-source"
# Match the immutable Go oracle's pre-native-Usage parent source.
parent_commit=abf20713bacdab562644256e27616a9dd7acb81e
evidence_dir="${output%.json}.evidence"
mkdir -p "$evidence_dir"
cleanup() {
  stage_exit=$?
  for receipt in go native input cli controls filesystem baseline-input baseline-go baseline-native owner owner-native owner-cli owner-controls path path-native argv argv-controls argv-build; do
    if [[ -f "$scratch/$receipt.json" ]]; then cp "$scratch/$receipt.json" "$evidence_dir/$receipt.json"; fi
  done
  if [[ $stage_exit != 0 ]]; then
    python3 - "$output" "$stage_exit" <<'PYFAIL'
import json, pathlib, subprocess, sys
pathlib.Path(sys.argv[1]).write_text(json.dumps({"schema_version":1,"gate_exit":int(sys.argv[2]),"candidate_head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"accepted":False},indent=2)+"\n")
PYFAIL
  fi
  git -C "$repo_root" worktree remove --force "$source_root" >/dev/null 2>&1 || true
  git -C "$repo_root" worktree remove --force "$parent_root" >/dev/null 2>&1 || true
  rm -rf "$scratch"
}
trap cleanup EXIT INT TERM
run_stage() {
  local label=$1
  shift
  if "$@" >"$evidence_dir/$label.log" 2>&1; then tail -n 12 "$evidence_dir/$label.log"; else local code=$?; tail -n 100 "$evidence_dir/$label.log" >&2; return "$code"; fi
}
cd "$repo_root"
python3 scripts/usage-copilot-kimi-oracle/cases.py "$scratch/input.json"
python3 scripts/usage-local-files-baseline/cases.py "$scratch/baseline-input.json"
mkdir -p "$scratch/home"
export USAGE_COPILOT_KIMI_ORACLE="$scratch/go.json" USAGE_COPILOT_KIMI_NATIVE="$scratch/native.json" USAGE_COPILOT_KIMI_INPUT="$scratch/input.json" USAGE_COPILOT_KIMI_ROOT="$scratch/home"
if command -v cygpath >/dev/null 2>&1; then
  export USAGE_COPILOT_KIMI_ORACLE=$(cygpath -w "$USAGE_COPILOT_KIMI_ORACLE") USAGE_COPILOT_KIMI_NATIVE=$(cygpath -w "$USAGE_COPILOT_KIMI_NATIVE") USAGE_COPILOT_KIMI_INPUT=$(cygpath -w "$USAGE_COPILOT_KIMI_INPUT") USAGE_COPILOT_KIMI_ROOT=$(cygpath -w "$USAGE_COPILOT_KIMI_ROOT")
fi
export USAGE_LOCAL_INPUT="$scratch/baseline-input.json" USAGE_LOCAL_OUTPUT="$scratch/baseline-go.json" USAGE_LOCAL_ROOT="$scratch/baseline-home" USAGE_COPILOT_KIMI_BASELINE_NATIVE="$scratch/baseline-native.json"
if command -v cygpath >/dev/null 2>&1; then
  export USAGE_LOCAL_INPUT=$(cygpath -w "$USAGE_LOCAL_INPUT") USAGE_LOCAL_OUTPUT=$(cygpath -w "$USAGE_LOCAL_OUTPUT") USAGE_LOCAL_ROOT=$(cygpath -w "$USAGE_LOCAL_ROOT") USAGE_COPILOT_KIMI_BASELINE_NATIVE=$(cygpath -w "$USAGE_COPILOT_KIMI_BASELINE_NATIVE")
fi
for label in OWNER OWNER_NATIVE PATH PATH_NATIVE; do
  name="USAGE_COPILOT_KIMI_$label"
  path="$scratch/$(echo "$label" | tr 'A-Z_' 'a-z-').json"
  if command -v cygpath >/dev/null 2>&1; then path=$(cygpath -w "$path"); fi
  export "$name=$path"
done
oracle_commit=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
git -C "$repo_root" worktree add --quiet --detach "$source_root" "$oracle_commit"
cp "$repo_root/scripts/usage-copilot-kimi-oracle/provider_test.go.txt" "$source_root/internal/usage/oracle_copilot_kimi_768_test.go"
cp "$repo_root/scripts/usage-local-files-baseline/provider_test.go.txt" "$source_root/internal/usage/oracle_local_files_baseline_768_test.go"
cp "$repo_root/scripts/usage-copilot-kimi-oracle/owner_test.go.txt" "$source_root/internal/usage/oracle_credential_owner_768_test.go"
export GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0
executable_suffix=$( [[ ${OS:-} == Windows_NT ]] && echo .exe || true )
run_stage go-build go -C "$source_root" build -trimpath -o "$scratch/go-usage$executable_suffix" ./cmd/symbrain
run_stage go-constructors go -C "$source_root" test ./internal/usage -run '^TestUsageCopilotKimi768$' -count=1
git -C "$source_root" diff --exit-code --quiet
run_stage go-original-baseline go -C "$source_root" test ./internal/usage -run '^TestUsageLocalFilesBaseline768$' -count=1
run_stage go-owners go -C "$source_root" test ./internal/usage -run '^TestUsageCredential(Owner|Path)768$' -count=1
run_stage native-constructors cargo test --locked -p symbrain-usage --lib copilot_kimi_oracle_matches_fresh_go -- --ignored --nocapture
# Stable parent/candidate binary hashes must not include incremental debug metadata.
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0
target_root=$(python3 -c 'import os; print(os.path.abspath(os.environ.get("CARGO_TARGET_DIR","target")))')
run_stage argv-initial-clean python3 "$repo_root/scripts/usage-copilot-kimi-oracle/clean_cli.py" "$target_root" "$repo_root" "$evidence_dir/argv-initial-clean.json" --with-usage
run_stage native-build cargo build --locked -p symbrain-cli
python3 - "$target_root/debug/symbrain$executable_suffix" "$scratch/argv-candidate-initial$executable_suffix" <<'PYCOPY'
import shutil,sys
shutil.copyfile(sys.argv[1],sys.argv[2]);shutil.copymode(sys.argv[1],sys.argv[2])
PYCOPY
run_stage cli python3 "$repo_root/scripts/usage-copilot-kimi-oracle/cli.py" "$scratch/go.json" "$scratch/go-usage$executable_suffix" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix" "$scratch/cli.json"
run_stage owner-cli python3 "$repo_root/scripts/usage-copilot-kimi-oracle/cli.py" "$scratch/owner.json" "$scratch/go-usage$executable_suffix" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix" "$scratch/owner-cli.json" --owner
run_stage controls python3 "$repo_root/scripts/usage-copilot-kimi-oracle/controls.py" "$scratch" "${CARGO_TARGET_DIR:-target}/debug/symbrain$executable_suffix"
# The frozen parent predates shared workspace API changes in the merge base.
# Archive and clean shared workspace packages between variants, not the full target.
git -C "$repo_root" worktree add --quiet --detach "$parent_root" "$parent_commit"
run_stage argv-parent-clean python3 "$repo_root/scripts/usage-copilot-kimi-oracle/clean_cli.py" "$target_root" "$repo_root" "$evidence_dir/argv-parent-clean.json" --with-usage
run_stage argv-parent-build cargo build --locked -p symbrain-cli --manifest-path "$parent_root/Cargo.toml" --target-dir "$target_root"
python3 - "$target_root/debug/symbrain$executable_suffix" "$scratch/argv-parent$executable_suffix" <<'PYCOPY'
import shutil,sys
shutil.copyfile(sys.argv[1],sys.argv[2]);shutil.copymode(sys.argv[1],sys.argv[2])
PYCOPY
run_stage argv-candidate-clean python3 "$repo_root/scripts/usage-copilot-kimi-oracle/clean_cli.py" "$target_root" "$repo_root" "$evidence_dir/argv-candidate-clean.json" --with-usage
run_stage argv-candidate-build cargo build --locked -p symbrain-cli --target-dir "$target_root"
python3 - "$parent_root" "$parent_commit" "$scratch/argv-parent$executable_suffix" "$target_root/debug/symbrain$executable_suffix" "$scratch/argv-build.json" "$scratch/argv-candidate-initial$executable_suffix" "$repo_root" <<'PYBUILD'
import hashlib,json,pathlib,subprocess,sys
root=pathlib.Path(sys.argv[1]);commit=sys.argv[2]
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip()==commit
assert not subprocess.check_output(['git','status','--porcelain'],cwd=root)
files=['Cargo.toml','Cargo.lock','rust/symbrain-cli/src/usage_cli.rs','rust/symbrain-cli/src/lib.rs']
manifest={}
for name in files:
    data=(root/name).read_bytes()
    assert data==subprocess.check_output(['git','show',commit+':'+name],cwd=root)
    manifest[name]=hashlib.sha256(data).hexdigest()
assert pathlib.Path(sys.argv[4]).read_bytes()==pathlib.Path(sys.argv[6]).read_bytes(),'Actual candidate binary restored byte-identically after parent variant'
candidate=pathlib.Path(sys.argv[7]);candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=candidate,text=True).strip()
candidate_clean=not subprocess.check_output(['git','status','--porcelain'],cwd=candidate)
candidate_manifest={name:hashlib.sha256((candidate/name).read_bytes()).hexdigest()for name in files}
if candidate_clean:
    for name in files:assert (candidate/name).read_bytes()==subprocess.check_output(['git','show',candidate_head+':'+name],cwd=candidate)
pathlib.Path(sys.argv[5]).write_text(json.dumps(dict(parent_source=commit,parent_source_clean=True,parent_source_sha256=manifest,candidate_source=candidate_head,candidate_source_clean=candidate_clean,candidate_source_sha256=candidate_manifest,candidate_restored_byte_identical=True,
    binaries_sha256={kind:hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()for kind,path in [('parent',sys.argv[3]),('rust',sys.argv[4])]},
    rust_sdk=subprocess.check_output(['rustc','-Vv'],text=True),scope='One exclusive target; every CLI and Usage dependency variant package-cleaned after verified lossless artifact archive. Actual parent copied; candidate restored byte-identically to actual initial candidate. Clean candidate source checked against immutable Git, no duplicate target'),indent=2)+'\n')
PYBUILD
run_stage argv python3 "$repo_root/scripts/usage-copilot-kimi-oracle/argv.py" --go "$scratch/go-usage$executable_suffix" --parent "$scratch/argv-parent$executable_suffix" --rust "$target_root/debug/symbrain$executable_suffix" --parent-source "$parent_commit" --output "$scratch/argv.json"
run_stage argv-controls python3 "$repo_root/scripts/usage-copilot-kimi-oracle/argv_controls.py" --go "$scratch/go-usage$executable_suffix" --parent "$scratch/argv-parent$executable_suffix" --rust "$target_root/debug/symbrain$executable_suffix" --parent-source "$parent_commit" --output "$scratch/argv-controls.json"
python3 "$repo_root/scripts/usage-copilot-kimi-oracle/receipt.py" "$scratch/go.json" "$scratch/native.json" "$output" "$oracle_commit"
