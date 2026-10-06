#!/usr/bin/env bash
# Actual frozen-Go baseline only; no native build, routing or parity claim.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
source "$repo_root/scripts/run-external-env.sh"
external_env
output=${1:?usage: run.sh OUTPUT_JSON}
output=$(python3 -c 'import os,sys;print(os.path.abspath(sys.argv[1]))' "$output")
scratch=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-local-baseline-768.XXXXXX")
source_root="$scratch/source"
evidence_dir="${output%.json}.evidence"
mkdir -p "$evidence_dir"
cleanup() {
  code=$?
  for name in input go; do if [[ -f "$scratch/$name.json" ]]; then cp "$scratch/$name.json" "$evidence_dir/$name.json"; fi; done
  git -C "$repo_root" worktree remove --force "$source_root" >/dev/null 2>&1 || true
  rm -rf "$scratch"
  return "$code"
}
trap cleanup EXIT
cd "$repo_root"
python3 scripts/usage-local-files-baseline/cases.py "$scratch/input.json"
export USAGE_LOCAL_INPUT="$scratch/input.json" USAGE_LOCAL_OUTPUT="$scratch/go.json" USAGE_LOCAL_ROOT="$scratch/home"
mkdir -p "$USAGE_LOCAL_ROOT"
if command -v cygpath >/dev/null 2>&1; then
  export USAGE_LOCAL_INPUT=$(cygpath -w "$USAGE_LOCAL_INPUT") USAGE_LOCAL_OUTPUT=$(cygpath -w "$USAGE_LOCAL_OUTPUT") USAGE_LOCAL_ROOT=$(cygpath -w "$USAGE_LOCAL_ROOT")
fi
oracle_commit=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
git worktree add --quiet --detach "$source_root" "$oracle_commit"
cp scripts/usage-local-files-baseline/provider_test.go.txt "$source_root/internal/usage/oracle_local_baseline_768_test.go"
export GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0
go -C "$source_root" test ./internal/usage -run '^TestUsageLocalFilesBaseline768$' -count=1 >"$evidence_dir/go-constructors.log" 2>&1
tail -n 12 "$evidence_dir/go-constructors.log"
git -C "$source_root" diff --exit-code --quiet
python3 - "$scratch/go.json" "$output" <<'PY'
import hashlib,json,pathlib,subprocess,sys
r=json.loads(pathlib.Path(sys.argv[1]).read_text(encoding='utf-8'));assert r['cases']==r['read_only_cases']==97 and len(r['records'])==len({x['id']for x in r['records']})==97
r['candidate_head']=subprocess.check_output(['git','rev-parse','HEAD'],text=True, encoding='utf-8').strip()
r['candidate_dirty']=bool(subprocess.check_output(['git','status','--porcelain'],text=True, encoding='utf-8').strip())
r['baseline_source_sha256']={str(p):hashlib.sha256(p.read_bytes()).hexdigest()for p in sorted(pathlib.Path('scripts/usage-local-files-baseline').glob('*'))if p.is_file()}
pathlib.Path(sys.argv[2]).write_text(json.dumps(r,indent=2)+'\n', encoding='utf-8')
PY
