"""Real CLI routing and credential-free output; no provider HTTP is started."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys
from reference_parity import reference_resolution_divergence

parser=argparse.ArgumentParser()
for name in ['fixture','go_binary','rust_binary','output']:parser.add_argument(name,type=pathlib.Path)
parser.add_argument('--control',choices=['exit','missing-case'])
# Go built from the candidate commit: an intentional Go change that Rust mirrors
# byte for byte (for example new usage help) is explained, never ignored.
parser.add_argument('--go-head',type=pathlib.Path)
args=parser.parse_args()
rows=json.loads(args.fixture.read_text(encoding='utf-8'))
assert len(rows)==86 and len({row['id']for row in rows})==86,'exact file corpus'
rows=[row for row in rows if not row['ambiguous']]
assert len(rows)==85,'one ambiguous source remains gated'
missing=[row for row in rows if not row['report']['providers'][0]['configured']]
expected=85+2*len(missing)
assert expected==151,'complete credential-free CLI corpus'
records=[]
for row in rows:
    home=pathlib.Path(row['home'])
    env={key:value for key,value in os.environ.items()if key in ['SYSTEMROOT','WINDIR','TEMP','TMP','TMPDIR','PATHEXT']}
    env.update({'HOME':str(home),'USERPROFILE':str(home),'CODEX_HOME':row['codex_home_override'],'CODEX_ACCESS_TOKEN':row['env_token']if row['provider']=='codex'else'','XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache'),'PATH':'','ANTHROPIC_OAUTH_TOKEN':'env://USAGE_FILES_ABSENT','SYMBRAIN_GO_BINARY':str(home/'absent-go')})
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
if args.control=='missing-case':records.pop()
report={'schema_version':1,'cases':len(records),'expected_cases':expected,'route_cases':85,'missing_file_report_cases':len(missing)*2,'passed':sum(row['matches']for row in records),'failed':sum(not row['matches']for row in records),'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in [('go',args.go_binary),('rust',args.rust_binary)]},'records':records,'claude_cli_keychain_isolation':'unresolved OAuth environment source preempts automatic system Keychain; full file constructor reports are separately verified with the private production injection point'}
report['byte_matches']=sum(row['byte_matches'] for row in records)
report['known_divergences']=sum(row['known_divergence'] for row in records)
args.output.write_text(json.dumps(report,indent=2)+'\n', encoding='utf-8')
if len(records)!=expected:sys.exit('provider file CLI oracle: missing CLI case')
if report['failed']:sys.exit('provider file CLI oracle: byte/exit mismatch: '+', '.join(row['id']for row in records if not row['matches']))
print('Provider file CLI oracle:',report['byte_matches'],'byte/exit matches;',report['known_divergences'],'known divergences (#930 or mirrored current Go)')
