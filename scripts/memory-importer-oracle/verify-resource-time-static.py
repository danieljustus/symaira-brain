"""Full-parent-index sparse source receipt. No SDK behavior or product runs."""
import argparse
import ast
import gzip
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PARENT = 'e55ea752b28d88df0b9d2248a8d32cb6d971f9e5'
PROOF = ROOT/'migration/evidence/memory-importers-761/resource-time-correction'
OUTPUT = PROOF/'source-map.json'


def git(*args): return subprocess.check_output(['git','-C',str(ROOT),*args])
def sha(raw): return hashlib.sha256(raw).hexdigest()


def build():
    assert not (ROOT/'target').exists()
    index = {}
    for row in git('ls-files','--stage','-z').split(b'\0'):
        if not row:continue
        meta,name=row.split(b'\t',1);mode,blob,stage=meta.decode().split();assert stage=='0'
        index[name.decode()]={'mode':mode,'blob':blob}
    parent = {}
    for row in git('ls-tree','-r','-z',PARENT).split(b'\0'):
        if not row:continue
        meta,name=row.split(b'\t',1);mode,kind,blob=meta.decode().split();assert kind=='blob'
        parent[name.decode()]={'mode':mode,'blob':blob}
    assert len(parent)==4257 and set(parent)<=set(index)
    allowed={'MIGRATION-RUST.md','migration/implementation-plan.md','migration/contract-matrix.csv',
             'rust/symbrain-memory/src/importer/markdown.rs','rust/symbrain-memory/src/importer/mod.rs'}
    changed={name for name in parent if parent[name]!=index[name]};assert changed==allowed
    materialized=[];source={}
    query=subprocess.Popen(['git','-C',str(ROOT),'cat-file','--batch'],stdin=subprocess.PIPE,stdout=subprocess.PIPE)
    for name,r in sorted(index.items()):
        if name==str(OUTPUT.relative_to(ROOT)):continue
        query.stdin.write((r['blob']+'\n').encode());query.stdin.flush();head=query.stdout.readline().split();assert head[1]==b'blob'
        raw=query.stdout.read(int(head[2]));assert query.stdout.read(1)==b'\n'
        path=ROOT/name
        if path.exists():
            assert path.read_bytes()==raw, name
            materialized.append({'path':name,'sha256':sha(raw),'bytes':len(raw),'allocated_bytes':path.stat().st_blocks*512})
        if name.endswith(('.rs','Cargo.toml')) or name=='Cargo.lock' or name.startswith('scripts/memory-importer-oracle/'):
            source[name]={**r,'sha256':sha(raw),'bytes':len(raw)}
    query.stdin.close();assert query.wait()==0
    assert sum(r['bytes']for r in materialized)<15*1024**2 and sum(r['allocated_bytes']for r in materialized)<15*1024**2
    assert not git('diff','--name-only')
    retention=json.loads((PROOF/'original-request-retention.json').read_bytes())
    assert len(retention['records'])==83
    for r in retention['records']:
        packed=(ROOT/r['retained_path']).read_bytes()
        assert sha(packed)==r['archive_sha256'] and sha(gzip.decompress(packed))==r['sha256']
        assert sha(Path(r['path']).read_bytes())==r['sha256']
    old=json.loads(git('show',PARENT+':migration/evidence/memory-importers-761/source-preparation/source-map.json'))
    frozen=Path('/workspace/oracles/daemon772-go-source')
    for name,digest in old['frozen_source'].items():assert sha((frozen/name).read_bytes())==digest
    for name,digest in old['sdk_source'].items():assert sha((Path('/workspace/toolchains/go1.26.7')/name).read_bytes())==digest
    modules={'modernc':Path('/home/agent/go/pkg/mod/modernc.org/sqlite@v1.59.0'),
             'sqlitekit':Path('/home/agent/go/pkg/mod/github.com/danieljustus/symaira-corekit@v0.17.0')}
    module_count=0
    for kind,rows in old['module_source'].items():
        for name,digest in rows.items():assert sha((modules[kind]/name).read_bytes())==digest;module_count+=1
    binding=json.loads((PROOF/'leap-resource-source-binding.json').read_bytes())
    for path,r in binding['records'].items():assert sha(Path(path).read_bytes())==r['sha256']
    original_units=sum((ROOT/('rust/symbrain-memory/src/'+name)).read_text().count('#[test]')
                       for name in ['importer/tests.rs','activity_retention_tests.rs'])
    assert original_units==32
    assert (ROOT/'rust/symbrain-memory/src/importer/resource_time_tests.rs').read_text().count('#[test]')==3
    python_files=list((ROOT/'scripts/memory-importer-oracle').glob('*.py'))
    for path in python_files:ast.parse(path.read_bytes())
    for path in (ROOT/'rust/symbrain-memory').rglob('*.rs'):
        if '/src/importer/'in str(path) or path.name in ['activity_retention.rs','activity_retention_tests.rs','retention761_oracle.rs']:
            assert len(path.read_text().splitlines())<400,path
    unchanged_criteria=['fixtures.py','corrections.py','correction_gate.py','compare.py','mutants.py','retention.go','retention_cases.py','state.py','verify-corrections-static.py']
    for name in unchanged_criteria:
        rel='scripts/memory-importer-oracle/'+name;assert (ROOT/rel).read_bytes()==git('show',PARENT+':'+rel)
    return {'status':'source-only-prepared-not-compiled-or-executed','parent':PARENT,'parent_paths':len(parent),
            'full_index_paths':len(index),'changed_parent_paths':sorted(changed),'parent_paths_unchanged':len(parent)-len(changed),
            'source_bindings':source,'materialized':materialized,'materialized_allocated_bytes':sum(x['allocated_bytes']for x in materialized),
            'self_map_excluded':str(OUTPUT.relative_to(ROOT)),'original_request_records_losslessly_verified':83,
            'frozen_source_files_verified':len(old['frozen_source']),'SDK_source_files_verified':len(old['sdk_source']),
            'module_source_files_verified':module_count,'leap_original_source_SDK_bindings_verified':len(binding['records']),
            'Python_AST_files':len(python_files),'unchanged_original_criteria_files':unchanged_criteria,
            'original_constructor_cases':93,'additive_constructor_cases':6,'total_prepared_constructor_cases':99,
            'unchanged_retention_recipes':12,'unchanged_unit_definitions':32,'additive_private_parser_tests':3,
            'original_actual_control_definitions':6,'additive_filename_domain_control':1,'runtime_executions':0,
            'SDK_behavior_probes':0,'dependencies_changed':False,'routing_admission':False,'native_three_OS':'pending'}


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args()
    actual=build()
    if args.write:OUTPUT.write_text(json.dumps(actual,indent=2,sort_keys=True)+'\n')
    else:assert json.loads(OUTPUT.read_bytes())==actual
    print(json.dumps({k:actual[k]for k in ['status','parent_paths','full_index_paths','parent_paths_unchanged','materialized_allocated_bytes','original_request_records_losslessly_verified','runtime_executions']}))


if __name__=='__main__':main()
