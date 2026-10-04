"""Sparse full-index source binding; no compiler/SDK behavior/product process."""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PARENT = "2946cf2b959ed71ed58dcf44d36c515aab60328e"
OUTPUT = ROOT/"migration/evidence/memory-importers-761/local-corrections/source-map.json"


def git(*args):
    return subprocess.check_output(["git", "-C", str(ROOT), *args])


def digest(data): return hashlib.sha256(data).hexdigest()


def build():
    assert not (ROOT/"target").exists()
    index = {}
    for record in git("ls-files", "--stage", "-z").split(b"\0"):
        if not record: continue
        meta, path = record.split(b"\t",1)
        mode, blob, stage = meta.decode().split()
        assert stage == "0"
        index[path.decode()] = {"mode":mode, "blob":blob}
    parent = {}
    for record in git("ls-tree", "-r", "-z", PARENT).split(b"\0"):
        if not record: continue
        meta, path = record.split(b"\t",1)
        mode, kind, blob = meta.decode().split()
        assert kind == "blob"
        parent[path.decode()] = {"mode":mode, "blob":blob}
    assert set(parent) <= set(index), "parent index path removed"
    changes = {name for name in parent if index[name] != parent[name]}
    allowed = {"rust/symbrain-memory/src/"+name for name in ["activity_retention.rs", "activity_retention_tests.rs",
        "importer/bytes.rs", "importer/files.rs", "importer/tests.rs"]}
    allowed |= {"MIGRATION-RUST.md", "migration/implementation-plan.md", "migration/contract-matrix.csv"}
    assert changes <= allowed, changes-allowed
    assert len(parent) == 4242
    materialized = [];source = {}
    query = subprocess.Popen(["git","-C",str(ROOT),"cat-file","--batch"],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
    for name, item in sorted(index.items()):
        if name == str(OUTPUT.relative_to(ROOT)):
            continue  # Self-referential receipt bytes are bound by the handoff.
        path = ROOT/name
        query.stdin.write((item["blob"]+"\n").encode());query.stdin.flush()
        header = query.stdout.readline().split();assert header[1] == b"blob"
        raw = query.stdout.read(int(header[2]));assert query.stdout.read(1) == b"\n"
        if path.exists():
            assert path.read_bytes() == raw, f"unstaged/materialized source: {name}"
            materialized.append({"path":name,"bytes":len(raw),"sha256":digest(raw)})
        if name.endswith((".rs","Cargo.toml")) or name == "Cargo.lock" or name.startswith("scripts/memory-importer-oracle/"):
            source[name] = {**item,"bytes":len(raw),"sha256":digest(raw)}
    query.stdin.close();assert query.wait() == 0
    assert sum(r['bytes'] for r in materialized) < 15*1024**2
    assert not git("diff", "--name-only"), "unstaged changes"
    preserved = ROOT/"migration/evidence/memory-importers-761/local-corrections/original-review-retention.json"
    import gzip
    retention = json.loads(preserved.read_bytes())
    for row in retention["records"]:
        packed = (ROOT/row["retained_path"]).read_bytes()
        assert digest(packed) == row["gzip_sha256"] and digest(gzip.decompress(packed)) == row["sha256"]
        assert digest(Path(row["original_path"]).read_bytes()) == row["sha256"]
    old_map = json.loads((ROOT/"migration/evidence/memory-importers-761/source-preparation/source-map.json").read_bytes())
    frozen = Path('/workspace/oracles/daemon772-go-source')
    assert subprocess.check_output(['git','-C',str(frozen),'rev-parse','HEAD']).decode().strip() == old_map['frozen_revision']
    assert not subprocess.check_output(['git','-C',str(frozen),'status','--porcelain'])
    for name, expected in old_map['frozen_source'].items():assert digest((frozen/name).read_bytes()) == expected
    sdk = Path('/workspace/toolchains/go1.26.7')
    for name, expected in old_map['sdk_source'].items():assert digest((sdk/name).read_bytes()) == expected
    modules = {"modernc":Path('/home/agent/go/pkg/mod/modernc.org/sqlite@v1.59.0'),
       "sqlitekit":Path('/home/agent/go/pkg/mod/github.com/danieljustus/symaira-corekit@v0.17.0')}
    for module, records in old_map['module_source'].items():
        for name, expected in records.items():assert digest((modules[module]/name).read_bytes()) == expected
    for p in (ROOT/'scripts/memory-importer-oracle').glob('*.py'):ast.parse(p.read_bytes())
    units = sum(p.read_text().count('#[test]') for p in [(ROOT/'rust/symbrain-memory/src/importer/tests.rs'),
        (ROOT/'rust/symbrain-memory/src/activity_retention_tests.rs')])
    assert units == 32
    for p in (ROOT/'rust/symbrain-memory').rglob('*.rs'):
        if p.name in ['activity_retention.rs','activity_retention_tests.rs','retention761_oracle.rs'] or '/src/importer/' in str(p):
            assert len(p.read_text().splitlines()) < 400
    return {'status':'source-only-full-index-bound-prepared-not-compiled-or-executed','parent':PARENT,
        'parent_paths':len(parent),'index_paths':len(index),'parent_paths_byte_unchanged':len(parent)-len(changes),
        'changed_parent_paths':sorted(changes),'native_and_harness_source':source,'materialized_files':materialized,
        'self_receipt_excluded_from_recursive_content_hash':str(OUTPUT.relative_to(ROOT)),
        'prepared_constructor_cases':93,'unchanged_original_constructor_cases':70,'additive_retention_cases':12,'prepared_units':32,
        'current_frozen_source_verified':len(old_map['frozen_source']),'sdk_source_verified':len(old_map['sdk_source']),
        'module_source_verified':6,'no_allocated_target':True,'actual_runtime_tests':0,'sdk_behavior_probes':0,
        'new_dependencies':[],'routing_admission':False,'native_three_os':'pending'}


def main():
    parser = argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    actual=build()
    if args.write: OUTPUT.write_text(json.dumps(actual,indent=2,sort_keys=True)+'\n')
    else: assert json.loads(OUTPUT.read_bytes()) == actual, 'index/source binding differs'
    print(json.dumps({k:actual[k] for k in ['status','parent_paths','index_paths','parent_paths_byte_unchanged',
        'current_frozen_source_verified','prepared_units','actual_runtime_tests']}))


if __name__ == '__main__':main()
