"""Real CLI routing and credential-free output; no provider HTTP is started."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys

parser=argparse.ArgumentParser()
for name in ['fixture','go_binary','rust_binary','output']:parser.add_argument(name,type=pathlib.Path)
parser.add_argument('--control',choices=['exit','missing-case'])
parser.add_argument('--owner',action='store_true')
args=parser.parse_args()
rows=json.loads(args.fixture.read_text(encoding='utf-8'))
owner=args.owner
expected=16 if owner else 89
assert len(rows)==expected and len({row['id']for row in rows})==expected,'exact Copilot/Kimi corpus'
rows=[row for row in rows if not row['gated']]
assert len(rows)==(16 if owner else 81 if os.name=='nt'else 82),'complete conservative file subset'
missing=[row for row in rows if not row['report']['providers'][0]['configured']]
expected=len(rows)+2*len(missing)
records=[]
for row in rows:
    before={path:pathlib.Path(path).read_bytes()for path in row['file_sha256']}
    for path,digest in row['file_sha256'].items():assert hashlib.sha256(before[path]).hexdigest()==digest,'source file hash'
    link=pathlib.Path(row['link'])if owner else None
    target=os.readlink(link)if owner else None
    home=pathlib.Path(row['home'])
    env={key:value for key,value in os.environ.items()if key in ['SYSTEMROOT','WINDIR','TEMP','TMP','TMPDIR','PATHEXT']}
    env.update({'HOME':row['home'],'USERPROFILE':row['userprofile'],'XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache'),'PATH':'','ANTHROPIC_OAUTH_TOKEN':'env://USAGE_LOCAL_FILES_ABSENT','SYMBRAIN_GO_BINARY':str(home/'absent-go')})
    env.update({key:value.replace('$HOME',row['home'])for key,value in row['env'].items()})
    variants=[('route',['--not-a-usage-flag'])]
    if row in missing:variants.extend([('table',[]),('json',['--json'])])
    for mode,flags in variants:
        observations=[]
        for binary in [args.go_binary,args.rust_binary]:
            result=subprocess.run([str(binary.resolve()),'usage',*flags],env=env,capture_output=True,check=False,timeout=15)
            observations.append({'exit':result.returncode,'stdout_b64':base64.b64encode(result.stdout).decode(),'stderr_b64':base64.b64encode(result.stderr).decode()})
        if args.control=='exit'and not records:observations[0]['exit']=42
        records.append({'id':row['id']+'-'+mode,'go':observations[0],'rust':observations[1],'matches':observations[0]==observations[1]})
    assert all(pathlib.Path(path).read_bytes()==value for path,value in before.items()),'actual CLI sources read-only'
    if owner:assert os.readlink(link)==target,'actual CLI owner symlink read-only'
if args.control=='missing-case':records.pop()
report={'schema_version':1,'cases':len(records),'expected_cases':expected,'route_cases':len(rows),'missing_file_report_cases':len(missing)*2,'passed':sum(row['matches']for row in records),'failed':sum(not row['matches']for row in records),'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in [('go',args.go_binary),('rust',args.rust_binary)]},'records':records,'read_only_source_cases':len(rows),'operator_keychain_isolation':'both CLIs seed unresolved OAuth; selected Copilot/Kimi Go constructors never invoke Claude Keychain'}
args.output.write_text(json.dumps(report,indent=2)+'\n', encoding='utf-8')
if len(records)!=expected:sys.exit('Copilot/Kimi CLI oracle: missing CLI case')
if report['failed']:sys.exit('Copilot/Kimi CLI oracle: byte/exit mismatch: '+', '.join(row['id']for row in records if not row['matches']))
print('Copilot/Kimi CLI oracle:',expected,'byte/exit matches')
