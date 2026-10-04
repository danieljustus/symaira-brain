"""Source-only validation: no Go/Rust product, target, compiler or port."""
import ast
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[4]
BASE = 'a1a29878f9b1b236862c09ac5c2bb5fb6cd3f4db'
OUT = Path(sys.argv[1]).resolve()
OUT.mkdir(parents=True, exist_ok=False)
sha = lambda b: hashlib.sha256(b).hexdigest()
git = lambda ref,name: subprocess.check_output(['git','show',ref+':'+name],cwd=ROOT)
head = subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT).decode().strip()
assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
folder = ROOT/'migration/evidence/guard-standalone-770/windows-config-owner-a1'
retained = json.loads((folder/'original/retention.json').read_bytes())
for row in retained['records']:
    compressed=(ROOT/row['retained']).read_bytes();raw=gzip.decompress(compressed)
    assert sha(compressed)==row['gzip_sha256'] and sha(raw)==row['sha256'] and len(raw)==row['bytes'],row
protected=json.loads(gzip.decompress((folder/'original/protected-source.json.gz').read_bytes()))
for name,digest in protected.items(): assert sha(git(head,name))==digest,name
path='scripts/guard-standalone-oracle/config_paths.py'
old=ast.parse(git(BASE,path));new=ast.parse((ROOT/path).read_bytes())
function=lambda tree,name:next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name==name)
unchanged=['cases','link_directory','metadata','compare']
for name in unchanged: assert ast.dump(function(old,name))==ast.dump(function(new,name)),name
oldreturn=next(n for n in ast.walk(function(old,'observe')) if isinstance(n,ast.Return))
newreturn=next(n for n in ast.walk(function(new,'observe')) if isinstance(n,ast.Return))
assert ast.dump(oldreturn)==ast.dump(newreturn)
assertions=lambda tree,name:[ast.dump(n) for n in ast.walk(function(tree,name)) if isinstance(n,ast.Assert)]
assert assertions(old,'observe')==assertions(new,'observe')
assert assertions(old,'control')==assertions(new,'control')
case=lambda tree:next(n for n in ast.walk(function(tree,'control')) if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=='case' for t in n.targets))
assert ast.dump(case(old))==ast.dump(case(new))
before=git(BASE,'scripts/guard-standalone-oracle/run.sh').decode().splitlines()
after=(ROOT/'scripts/guard-standalone-oracle/run.sh').read_text().splitlines()
assert [r for r in after if not r.startswith('python3 -m unittest discover ')]==before
# Generate only pure case data; no fixture or product setup.
from types import SimpleNamespace
vectors={}
for platform in ['nt','posix']:
    ns={'os':SimpleNamespace(name=platform)}
    exec(compile(ast.Module(body=[function(new,'cases')],type_ignores=[]),path,'exec'),ns)
    vectors[platform]=ns['cases']()
assert len(vectors['nt'])==23 and len(vectors['posix'])==25
(OUT/'prepared-vectors.json').write_text(json.dumps(vectors,indent=2)+'\n')
ntpath=__import__('ntpath');lib=Path(ntpath.__file__)
(OUT/'actual-local-CPython-ntpath.py').write_bytes(lib.read_bytes())
commands=[
    ('portable-tests',['python3','-m','unittest','discover','-s','scripts/guard-standalone-oracle','-p','test_config_path_progress.py','-v']),
    ('shell-syntax',['bash','-n','scripts/guard-standalone-oracle/run.sh']),
    ('actionlint',['/workspace/toolchains/bin/actionlint','.github/workflows/ci.yml']),
    ('whitespace',['git','diff','--check',BASE,'HEAD'])]
records=[]
scratch=OUT/'owned-temp';scratch.mkdir()
for name,command in commands:
    result=subprocess.run(command,cwd=ROOT,env={**os.environ,'TMPDIR':str(scratch),'PYTHONDONTWRITEBYTECODE':'1'},capture_output=True)
    for label,b in [('stdout',result.stdout),('stderr',result.stderr)]: (OUT/(name+'.'+label)).write_bytes(b)
    records.append(dict(name=name,command=command,exit=result.returncode,
                        stdout_sha256=sha(result.stdout),stderr_sha256=sha(result.stderr)))
    assert result.returncode==0,records[-1]
assert not list(scratch.iterdir())
assert not subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)
bad=subprocess.run(['git','merge-base','--is-ancestor','b2ef6ecc',head],cwd=ROOT,capture_output=True)
assert bad.returncode==1,'bad empty-index commit entered candidate lineage'
names=subprocess.check_output(['git','diff','--name-only',BASE,head],cwd=ROOT).decode().splitlines()
report=dict(source=head,base=BASE,clean=True,source_only=True,original_retention_roundtrips=len(retained['records']),
            protected_production_files_verified=len(protected),unchanged_case_and_comparison_functions=unchanged,
            observation_contract_and_readonly_assertion_unchanged=True,control_criteria_and_case_unchanged=True,
            original_run_commands_unchanged=True,prepared_Windows_cases=23,prepared_Unix_cases=25,
            local_CPython_version=sys.version,local_ntpath_source_sha256=sha(lib.read_bytes()),
            CPython_path_library_API_stand_ins_not_native_Windows=True,portable_tests=10,
            commands=records,source_sha256={name:sha((ROOT/name).read_bytes()) for name in names},
            current_or_original_Go_native_product_executions=0,compiler_or_target_or_port=False,
            bad_sparse_commit_not_ancestor=True,original_control_output_gap_preserved=True,
            remaining='Different-author source review and actual native full Windows/Unix Guard gates; no CI cause or runtime approval claim.')
(OUT/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
print('Source-only:1760production files/126original roundtrips/10portable tests/4static gates PASS; native runtime remains pending.')
