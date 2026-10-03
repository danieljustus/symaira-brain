#!/usr/bin/env bash
# Native process proof plus the immutable Go report's unchanged core fields.
set -euo pipefail
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
work_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/skills-status-621.XXXXXX")"
trap 'rm -rf "$work_dir"' EXIT
go_binary="$work_dir/symbrain-go"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$go_binary.exe"
  go_binary="$(cygpath -m "$go_binary")"
fi
report="${1:?usage: scripts/skills-render-drift/run.sh OUTPUT_JSON}"
./scripts/run-go-oracle.sh "$oracle_ref" build -o "$go_binary" ./cmd/symbrain
SYMBRAIN_SKILLS_STATUS_GO_ORACLE="$go_binary" \
SYMBRAIN_SKILLS_STATUS_REQUIRE_ORACLE=1 \
SYMBRAIN_SKILLS_STATUS_EVIDENCE="$report" \
cargo test -p symbrain-cli --test skills_render_drift --locked -- --nocapture
python3 - "$report" "$oracle_ref" "$go_binary" <<'PY'
import hashlib, json, platform, subprocess, sys
from pathlib import Path
report, ref, binary = sys.argv[1:]
path = Path(report)
data = json.loads(path.read_text())
assert data['go_oracle_ref'] == ref and data['total'] == len(data['cases']) == 12
assert all(case['existing_report_matches'] for case in data['cases'])
assert len({(case['target'], case['state']) for case in data['cases']}) == 12
data['runtime'] = platform.platform()
data['candidate_head'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
data['candidate_dirty'] = bool(subprocess.check_output(['git', 'status', '--porcelain'], text=True).strip())
data['go_binary_sha256'] = hashlib.sha256(Path(binary).read_bytes()).hexdigest()
names = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', ref, '--',
                                'cmd/symbrain', 'internal/skills', 'internal/paths', 'internal/config', 'go.mod', 'go.sum'], text=True).splitlines()
data['go_source_sha256'] = {name: hashlib.sha256(subprocess.check_output(['git', 'show', f'{ref}:{name}'])).hexdigest() for name in names if name.endswith('.go') or name in ('go.mod', 'go.sum')}
data['candidate_source_sha256'] = {name: hashlib.sha256(Path(name).read_bytes()).hexdigest() for name in (
    'rust/symbrain-cli/src/skills_cli.rs', 'rust/symbrain-skills/src/install/status.rs',
    'rust/symbrain-skills/src/install/status_compare.rs', 'rust/symbrain-skills/src/install/render_status.rs',
    'rust/symbrain-skills/src/install/drift.rs',
    'rust/symbrain-cli/tests/skills_render_drift.rs', 'rust/symbrain-skills/tests/render_drift_status.rs',
    'scripts/skills-render-drift/run.sh')}
data['intentional_extensions'] = ['render_status', 'render_drift', 'render_error', 'verified symlink presentation mode: linked']
data['comparison'] = 'actual process success, empty stderr, full existing report equality after removing only the explicit #621 extensions'
path.write_text(json.dumps(data, indent=2) + '\n')
print('12/12 live native CLI reports preserve the immutable Go report and verify the #621 extensions')
PY
