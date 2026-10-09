"""Whole Rust workspace source attribution only; never builds or executes it."""
import gzip
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent


def sha(raw): return hashlib.sha256(raw).hexdigest()


def main():
    head = subprocess.check_output(['git','-C',str(ROOT),'rev-parse','HEAD'],text=True).strip()
    names = subprocess.check_output(['git','-C',str(ROOT),'ls-files','rust'],text=True).splitlines()
    rows = {}
    for name in names:
        raw = (ROOT/name).read_bytes()
        assert raw == subprocess.check_output(['git','-C',str(ROOT),'show',head+':'+name])
        rows[name] = {'bytes':len(raw),'sha256':sha(raw)}
    value = {'kind':'ALL_WORKSPACE_RUST_SOURCE_BINDINGS_NOT_COMPILER_OR_TEST_SUCCESS',
             'source_head':head,'files':rows,'file_count':len(rows),'runtime_or_compiler_executions':0}
    raw = (json.dumps(value,indent=2)+'\n').encode()
    archive = OUT/'all-workspace-rust-sources.json.gz'
    packed = gzip.compress(raw,mtime=0); archive.write_bytes(packed)
    assert gzip.decompress(packed)==raw
    summary = {'archive':archive.name,'archive_bytes':len(packed),'archive_sha256':sha(packed),
               'payload_bytes':len(raw),'payload_sha256':sha(raw),'source_head':head,'files':len(rows)}
    (OUT/'all-workspace-rust-retention.json').write_text(json.dumps(summary,indent=2)+'\n')
    print(json.dumps(summary))


if __name__=='__main__': main()
