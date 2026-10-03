#!/usr/bin/env python3
"""Exercise failure controls through the actual native process fixture test."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
ROOT = Path(__file__).resolve().parents[2]


def main():
    fixture = json.loads((ROOT/'rust/symbrain-cli/tests/fixtures/activity_validation_767.json').read_text())
    controls = []
    with tempfile.TemporaryDirectory(prefix='activity-controls-767-') as temp:
        root = Path(temp)
        mutated = json.loads(json.dumps(fixture))
        mutated['cases'][0]['exit'] = 42
        shortened = json.loads(json.dumps(fixture))
        shortened['cases'].pop(0)
        for name, data, diagnostic in [('missing-fixture', None, 'required activity validation fixture'), ('changed-Go-exit', mutated, 'exit'), ('missing-case', shortened, 'complete activity process case count')]:
            path = root/f'{name}.json'
            if data is not None:
                path.write_text(json.dumps(data))
            env = dict(os.environ, SYMBRAIN_ACTIVITY_VALIDATION_FIXTURE=str(path))
            output = subprocess.run(['cargo','test','-p','symbrain-cli','--test','activity_validation_tests','--locked'],cwd=ROOT,env=env,capture_output=True,text=True,timeout=120)
            passed = output.returncode != 0 and diagnostic in output.stdout+output.stderr
            controls.append(dict(name=name,exit=output.returncode,expected_diagnostic=diagnostic,rejected=passed))
            assert passed, f'{name}: failure control did not reject for intended reason'
    Path(sys.argv[1]).write_text(json.dumps(dict(controls=controls),indent=2)+'\n')
    print('3/3 actual native fixture failure controls passed')
    return 0

if __name__=='__main__':
    raise SystemExit(main())
