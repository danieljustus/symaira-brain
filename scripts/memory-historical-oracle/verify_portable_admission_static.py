"""Complete parent/source/archive binding; no C/SQL/SDK/product behavior runs."""
import argparse
import ast
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
PARENT = 'a5c65c706ac4cb271fde26d9063b0f4afff1b420'
PROOF = ROOT/'migration/evidence/memory-historical-649/portable-admission-fixture'
OUTPUT = PROOF/'source-map.json'
FIXTURE = 'scripts/memory-historical-oracle/test_provider_admission.py'


def sha(raw): return hashlib.sha256(raw).hexdigest()
def git(*args): return subprocess.check_output(['git','-C',str(ROOT),*args])


def methods(raw):
    result = {}
    for node in ast.parse(raw).body:
        if isinstance(node,ast.ClassDef):
            for method in node.body:
                if isinstance(method,ast.FunctionDef) and method.name.startswith('test_'):
                    result[node.name+'.'+method.name] = ast.dump(method,include_attributes=False)
    return result


def build():
    assert not (ROOT/'target').exists()
    parent={}
    for line in git('ls-tree','-r','-z',PARENT).split(b'\0'):
        if not line:continue
        meta,name=line.split(b'\t',1);mode,kind,blob=meta.decode().split();assert kind=='blob'
        parent[name.decode()]={'mode':mode,'blob':blob}
    index={}
    for line in git('ls-files','--stage','-z').split(b'\0'):
        if not line:continue
        meta,name=line.split(b'\t',1);mode,blob,stage=meta.decode().split();assert stage=='0'
        index[name.decode()]={'mode':mode,'blob':blob}
    assert len(parent)==4829 and set(parent)<=set(index)
    changed={name for name in parent if parent[name]!=index[name]};assert changed=={FIXTURE}
    assert not git('diff','--name-only')
    materialized=[]
    for path in ROOT.rglob('*'):
        if not path.is_file() or path.name=='.git' or path==OUTPUT:continue
        name=str(path.relative_to(ROOT));assert name in index,name
        raw=path.read_bytes();assert raw==git('show',':'+name),name
        materialized.append({'path':name,'bytes':len(raw),'sha256':sha(raw),'allocated_bytes':path.stat().st_blocks*512})
    assert sum(x['bytes']for x in materialized)<15*1024**2
    assert sum(x['allocated_bytes']for x in materialized)<15*1024**2
    old_methods={};current_methods={}
    for name in ['test_provider.py','test_provider_admission.py']:
        rel='scripts/memory-historical-oracle/'+name
        old_methods.update(methods(git('show',PARENT+':'+rel)))
        current_methods.update(methods((ROOT/rel).read_bytes()))
    assert len(old_methods)==14 and current_methods==old_methods
    additional=methods((ROOT/'scripts/memory-historical-oracle/test_provider_platform.py').read_bytes())
    assert len(additional)==3
    new_tree=ast.parse((ROOT/'scripts/memory-historical-oracle/test_provider_platform.py').read_bytes())
    assert not any(isinstance(n,ast.ImportFrom) and any(a.name=='ProviderAdmission'for a in n.names)for n in new_tree.body), 'imported TestCase duplicates unittest discovery'
    python_files=list((ROOT/'scripts/memory-historical-oracle').glob('*.py'))
    for path in python_files:ast.parse(path.read_bytes())
    retained=json.loads((PROOF/'original-retention.json').read_bytes());assert retained['own_records']==58 and retained['prior_input_bindings']==422
    for row in retained['physical_records']:
        p=Path(row['original_path']);raw=p.read_bytes();assert sha(raw)==row['sha256'] and len(raw)==row['bytes'];st=p.lstat()
        assert (st.st_mode,st.st_uid,st.st_gid,st.st_mtime_ns)==(row['mode'],row['uid'],row['gid'],row['mtime_ns'])
        if 'archive'in row:
            packed=Path(row['archive']).read_bytes();assert sha(packed)==row['archive_sha256'] and gzip.decompress(packed)==raw
        else:assert row['retained_existing_original_payload']==str(p)
    for row in retained['immutable_Git_records']:
        raw=git('show',row['git_reference']);assert sha(raw)==row['sha256'] and len(raw)==row['bytes']
        ref,name=row['git_reference'].split(':',1);info=git('ls-tree',ref,'--',name).decode().split('\t')[0].split();assert info==[row['mode'],'blob',row['blob']]
    archive=ROOT/retained['own_full_review_tar'];assert sha(archive.read_bytes())==retained['own_full_review_tar_sha256']
    own=retained['physical_records'][:59];assert len(own)==59
    prefix='/workspace/review-artifacts/memory803-a5c65-provider-review-doctor/'
    with tarfile.open(fileobj=io.BytesIO(gzip.decompress(archive.read_bytes())))as tar:
        assert len(tar.getmembers())==59
        for row in own:
            assert row['original_path'].startswith(prefix)
            member=tar.getmember(row['original_path'][len(prefix):]);raw=tar.extractfile(member).read();assert sha(raw)==row['sha256']
            assert (member.mode,member.uid,member.gid)==(row['mode']&0o7777,row['uid'],row['gid'])
            assert member.pax_headers['mtime']==f"{row['mtime_ns']//10**9}.{row['mtime_ns']%10**9:09d}"
    for path in [ROOT/FIXTURE,ROOT/'scripts/memory-historical-oracle/test_provider_platform.py',Path(__file__)]:assert len(path.read_text().splitlines())<400
    return {'status':'source-only-all-new-mocked-controls-prepared-not-executed','parent':PARENT,
            'full_index_paths':len(index),'parent_paths_retained':len(parent),'parent_paths_unchanged':len(parent)-len(changed),
            'changed_parent_paths':sorted(changed),'materialized':materialized,'materialized_allocated_bytes':sum(x['allocated_bytes']for x in materialized),
            'original_test_method_ASTs_unchanged':14,'additive_mock_test_methods':3,'nested_missing_constant_suite_original_methods':6,
            'original_own_review_records':58,'original_receipt_extra_records':1,'original_prior_input_bindings':422,
            'physical_native_metadata_records_verified':len(retained['physical_records']),
            'immutable_Git_records_verified':len(retained['immutable_Git_records']),'own_full_review_tar_members':59,
            'Python_AST_files':len(python_files),'production_and_workflow_changes':0,'dependencies_changed':False,
            'original37_SQL_and9_controls_unchanged':True,'mock_tests_executed':0,'native_or_SDK_behavior_executions':0,
            'compiler_target_cache_ports_GitHub':0,'native_three_OS':'pending','self_map_excluded':str(OUTPUT.relative_to(ROOT))}


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--write',action='store_true');args=parser.parse_args();actual=build()
    if args.write:OUTPUT.write_text(json.dumps(actual,indent=2,sort_keys=True)+'\n')
    else:assert json.loads(OUTPUT.read_bytes())==actual
    print(json.dumps({k:actual[k]for k in ['status','full_index_paths','parent_paths_retained','parent_paths_unchanged','original_test_method_ASTs_unchanged','original_prior_input_bindings','materialized_allocated_bytes','native_or_SDK_behavior_executions']}))


if __name__=='__main__':main()
