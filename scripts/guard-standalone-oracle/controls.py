#!/usr/bin/env python3
"""Reject actual native-process mutants at the newly ported diagnostic boundaries."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import replay

go, native, output = map(Path, sys.argv[1:])
go, native = go.resolve(strict=True), native.resolve(strict=True)
selected = {case["id"]: case for case in replay.cases()}
controls = [("hide-config-error", "doctor-invalid-default"),
            ("hide-anchor-error", "doctor-anchor-count-overflow"),
            ("allow-without-audit", "decide-audit-failure")]
results = []
with tempfile.TemporaryDirectory(prefix="guard770-diagnostic-controls-") as owned:
    root = Path(owned)
    for index, (mutation, case_id) in enumerate(controls):
        base = root / str(index)
        base.mkdir()
        wrapper = base / "mutant.py"
        wrapper.write_text("""import json,re,subprocess,sys
p=subprocess.run([NATIVE,*sys.argv[1:]],input=sys.stdin.buffer.read(),capture_output=True)
assert p.returncode==0 if MODE=='allow-without-audit' else p.returncode==1
assert not p.stderr
raw=p.stdout
if MODE=='allow-without-audit':
    value=json.loads(raw);assert value['decision']=='deny';value['decision']='allow'
    raw=(json.dumps(value,separators=(',',':'))+'\\n').encode()
else:
    name='config' if MODE=='hide-config-error' else 'audit log'
    raw,count=re.subn(rb'(?m)^(  '+name.encode()+rb' +)error: [^\\n]+$',rb'\\1ok',raw)
    assert count==1
sys.stdout.buffer.write(raw);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)
""".replace("NATIVE", repr(str(native))).replace("MODE", repr(mutation)))
        case = selected[case_id]
        left = replay.observe(go, case, base / "go", False)
        right = replay.observe(wrapper, case, base / "rust", True)
        try:
            replay.compare(case, left, right)
        except AssertionError:
            results.append(dict(id=mutation, case=case_id, rejected=True, go=left, mutated_native=right,
                                wrapper_sha256=hashlib.sha256(wrapper.read_bytes()).hexdigest()))
        else:
            raise AssertionError("mutant accepted: " + mutation)
report = dict(candidate_head=subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=replay.ROOT, text=True).strip(),
              candidate_dirty=bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=replay.ROOT)),
              rejected=len(results), controls=results,
              native_sha256=hashlib.sha256(native.read_bytes()).hexdigest(),
              go_sha256=hashlib.sha256(go.read_bytes()).hexdigest(),
              controls_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest())
output.write_text(json.dumps(report, indent=2) + "\n")
print("actual diagnostic output mutants rejected:", len(results))
