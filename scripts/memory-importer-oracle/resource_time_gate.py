"""PREPARED full99 constructor/read-only gate and real filename-domain mutant.

Original93/12/32/six controls remain mandatory through correction_gate.py and
parent runners. This additive driver never compiles or grants a route admission.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import resource_time_cases
import state
from correction_gate import run


def main():
    assert os.environ.get("IMPORTER761_RUNTIME_ALLOCATED") == "1", "explicit Root allocation required"
    parser = argparse.ArgumentParser()
    for key in ("go-importer", "native-importer", "work-root"):
        parser.add_argument("--"+key, required=True, type=Path)
    args = parser.parse_args()
    root = args.work_root.resolve()
    assert root.is_relative_to('/workspace') and not root.exists()
    root.mkdir(mode=0o700)
    env = {k:v for k,v in os.environ.items() if not k.startswith(("SYMBRAIN_", "SYMMEMORY_", "OPENAI_", "ANTHROPIC_"))}
    env.update(HOME=str(root/"home"), XDG_CONFIG_HOME=str(root/"config"), XDG_DATA_HOME=str(root/"data"), TZ="UTC")
    for name in ("home", "config", "data"):(root/name).mkdir()
    here = Path(__file__).resolve().parent
    run([sys.executable,here/"corrections.py",root/"fixtures"],root/"materialize",env)
    original_input = root/"fixtures/input.json"
    original93 = json.loads(original_input.read_bytes());assert len(original93)==93
    original_sha = hashlib.sha256(original_input.read_bytes()).hexdigest()
    packets = resource_time_cases.append(root/"fixtures",original93)
    expanded = root/"input99.json";expanded.write_text(json.dumps(packets,indent=2)+"\n")
    before = state.snapshot(root/"fixtures")
    go = run([args.go_importer,expanded],root/"go99",env)
    native = run([args.native_importer,expanded],root/"native99",env)
    assert state.snapshot(root/"fixtures")==before, "constructors wrote owned sources"
    assert hashlib.sha256(original_input.read_bytes()).hexdigest()==original_sha
    run([sys.executable,here/"compare.py",expanded,root/"go99.stdout",root/"native99.stdout"],root/"compare99",env)
    # Go equality compares complete ordered raw metadata, errors, discovery and
    # direct imports of discovered documents; no timestamp normalization.
    target = next(p for p in packets if p['Id'].endswith('-invalid-6h'))
    original = Path(os.fsdecode(bytes.fromhex(target['RootHex'])))
    old = original/'extensions/owned/resources/2026-01-01T23-59-60-owned-6h-context.md'
    new = old.with_name(old.name.replace('23-59-60','23-59-59'))
    parent_stat = old.parent.stat()
    old.rename(new)
    try:
        mutated_before = state.snapshot(root/'fixtures')
        raw = run([args.native_importer,expanded],root/'mutant99',env)
        assert state.snapshot(root/'fixtures')==mutated_before
    finally:
        new.rename(old)
        os.utime(old.parent,ns=(parent_stat.st_atime_ns,parent_stat.st_mtime_ns))
    assert state.snapshot(root/'fixtures')==before, "owned filename mutant was not restored"
    rows = list(map(json.loads,raw.splitlines()));baseline = list(map(json.loads,native.splitlines()))
    assert [r['id']for r in rows]==[p['Id']for p in packets]
    changed=[a['id']for a,b in zip(rows,baseline,strict=True)if a!=b]
    assert changed==[target['Id']], "mutant bootstrap/scope failure"
    def granularity(record):
        assert len(record['sessions'])==1
        metadata=dict(record['sessions'][0]['metadata_hex'])
        return bytes.fromhex(metadata[b'granularity'.hex()])
    assert granularity(next(r for r in baseline if r['id']==target['Id']))==b'consolidated'
    assert granularity(next(r for r in rows if r['id']==target['Id']))==b'6h'
    rejected = subprocess.run([sys.executable,str(here/'compare.py'),str(expanded),str(root/'go99.stdout'),str(root/'mutant99.stdout')],env=env,capture_output=True)
    (root/'mutant-rejection.stdout').write_bytes(rejected.stdout);(root/'mutant-rejection.stderr').write_bytes(rejected.stderr)
    assert rejected.returncode!=0 and b'full constructor report mismatch' in rejected.stderr
    receipt={'status':'actual full99 paired reports equal and one filename-domain mutant rejected','original_cases_unchanged':93,'additive_cases':6,'original93_input_sha256':original_sha,'semantic_boundaries':resource_time_cases.original_boundaries(),'readonly_state':before,'actual_mutation_changed_ids':changed,'normalizations':[]}
    (root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')


if __name__=='__main__':main()
