#!/usr/bin/env python3
"""Retain the actual freshly built executables before the owned Go tree expires."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

go, native, report = map(Path, sys.argv[1:])
root = Path(__file__).resolve().parents[2]
output = report.parent/(report.stem+'-binaries')
output.mkdir(parents=True, exist_ok=True)
records = []
for name, original in [('go', go), ('native', native)]:
    original = original.resolve(strict=True)
    destination = output/(name+original.suffix)
    shutil.copyfile(original, destination)
    raw = original.read_bytes()
    assert destination.read_bytes() == raw, 'actual executable retention failed'
    records.append(dict(role=name, original=str(original), retained=str(destination.resolve()),
                        sha256=hashlib.sha256(raw).hexdigest(), bytes=len(raw)))
receipt = dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'], cwd=root, text=True).strip(),
               candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'], cwd=root)),
               records=records, roundtrip_verified=True,
               note='These exact binaries execute the following gates; frozen Go source/SDK and full candidate manifests bind the main process receipt')
(output/'receipt.json').write_text(json.dumps(receipt, indent=2)+'\n', encoding='utf-8')
