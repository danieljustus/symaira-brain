"""Real CLI routing and credential-free output; no provider HTTP is started."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'usage-provider-files-oracle'))
from reference_parity import reference_resolution_divergence

parser=argparse.ArgumentParser()
for name in ['fixture','go_binary','rust_binary','output']:parser.add_argument(name,type=pathlib.Path)
parser.add_argument('--control',choices=['exit','missing-case'])
parser.add_argument('--owner',action='store_true')
# Go built from the candidate commit: an intentional Go change that Rust mirrors
# byte for byte (for example new usage help) is explained, never ignored.
parser.add_argument('--go-head',type=pathlib.Path)
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
        def observe(binary):
            result=subprocess.run([str(binary.resolve()),'usage',*flags],env=env,capture_output=True,check=False,timeout=15)
            return {'exit':result.returncode,'stdout_b64':base64.b64encode(result.stdout).decode(),'stderr_b64':base64.b64encode(result.stderr).decode()}
        observations=[observe(args.go_binary),observe(args.rust_binary)]
        if args.control=='exit'and not records:observations[0]['exit']=42
        byte_matches=observations[0]==observations[1]
        known_divergence=not byte_matches and reference_resolution_divergence(*observations)
        head=None
        if not byte_matches and not known_divergence and args.go_head:
            head=observe(args.go_head)
            known_divergence=head!=observations[0] and head==observations[1]
        records.append({'id':row['id']+'-'+mode,'go':observations[0],'rust':observations[1],'matches':byte_matches or known_divergence,'byte_matches':byte_matches,'known_divergence':known_divergence,**({'go_head':head} if head else {})})
    assert all(pathlib.Path(path).read_bytes()==value for path,value in before.items()),'actual CLI sources read-only'
    if owner:assert os.readlink(link)==target,'actual CLI owner symlink read-only'
if args.control=='missing-case':records.pop()
report={'schema_version':1,'cases':len(records),'expected_cases':expected,'route_cases':len(rows),'missing_file_report_cases':len(missing)*2,'passed':sum(row['matches']for row in records),'failed':sum(not row['matches']for row in records),'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in [('go',args.go_binary),('rust',args.rust_binary)]},'records':records,'read_only_source_cases':len(rows),'operator_keychain_isolation':'both CLIs seed unresolved OAuth; selected Copilot/Kimi Go constructors never invoke Claude Keychain'}
report['byte_matches']=sum(row['byte_matches'] for row in records)
report['known_divergences']=sum(row['known_divergence'] for row in records)
args.output.write_text(json.dumps(report,indent=2)+'\n', encoding='utf-8')
if len(records)!=expected:sys.exit('Copilot/Kimi CLI oracle: missing CLI case')
if report['failed']:sys.exit('Copilot/Kimi CLI oracle: byte/exit mismatch: '+', '.join(row['id']for row in records if not row['matches']))
print('Copilot/Kimi CLI oracle:',report['byte_matches'],'byte/exit matches;',report['known_divergences'],'known #930 divergences')
