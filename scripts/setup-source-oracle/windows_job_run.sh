#!/usr/bin/env bash
# Native diagnostic only; never changes Source's original 25-second gate.
set -euo pipefail
umask 022
[ "${OS:-}" = Windows_NT ] || { echo "Native Windows diagnostic required" >&2; exit 1; }
report="${1:?usage: scripts/setup-source-oracle/windows_job_run.sh OUTPUT_JSON}"
build_report="$report.cargo.jsonl"
cargo test -p symbrain-cli --test source_job_notifications --all-features --no-run --locked --message-format=json \
  2>&1 | tee "$build_report"
binary="$(python3 - "$build_report" <<'PY'
import json,sys
from pathlib import Path
artifacts=[]
for line in Path(sys.argv[1]).read_text().splitlines():
    try: row=json.loads(line)
    except json.JSONDecodeError: continue
    target=row.get("target",{})
    if row.get("reason")=="compiler-artifact" and target.get("name")=="source_job_notifications" and "test" in target.get("kind",[]) and row.get("executable"):
        assert Path(target["src_path"]).resolve()==Path("rust/symbrain-cli/tests/source_job_notifications.rs").resolve()
        artifacts.append(Path(row["executable"]).resolve())
assert len(artifacts)==1, f"Expected one exact source-bound diagnostic test artifact, got {artifacts}"
assert artifacts[0].is_file()
print(artifacts[0])
PY
)"
python3 scripts/setup-source-oracle/windows_job_probe.py "$binary" "$report"
