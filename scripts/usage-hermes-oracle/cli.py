"""Actual CLI bytes for invalid/expired Hermes stores without HTTP requests."""
import argparse
import base64
import hashlib
import json
import os
import pathlib
import subprocess
import sys

from hermes_compare import hermes_login_divergence
sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / 'usage-provider-files-oracle'))
from reference_parity import normalize_reference_resolution, reference_resolution_divergence

parser=argparse.ArgumentParser()
parser.add_argument("fixture",type=pathlib.Path)
parser.add_argument("go_binary",type=pathlib.Path)
parser.add_argument("rust_binary",type=pathlib.Path)
parser.add_argument("output",type=pathlib.Path)
parser.add_argument("--control",choices=["exit","missing-case"])
args=parser.parse_args()
rows=json.loads(args.fixture.read_text(encoding='utf-8'))
rows=[row for row in rows if not row['report']['providers'][0]['configured']]
assert len(rows)==52,"exact invalid/expired Go-selected corpus"
records=[]

for row in rows:
    home=pathlib.Path(row['home'])
    env={key:value for key,value in os.environ.items()if key in ['SYSTEMROOT','WINDIR','TEMP','TMP','TMPDIR','PATHEXT']}
    env.update({'HOME':str(home),'USERPROFILE':str(home),'HERMES_HOME':row['hermes_home'],'XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache'),'PATH':'','ANTHROPIC_OAUTH_TOKEN':'env://USAGE_HERMES_ABSENT','SYMBRAIN_GO_BINARY':str(home/'absent-go')})
    for mode,flags in [('table',[]),('json',['--json'])]:
        observations=[]
        for binary in [args.go_binary,args.rust_binary]:
            result=subprocess.run([str(binary.resolve()),'usage',*flags],env=env,capture_output=True,check=False,timeout=15)
            observations.append({'exit':result.returncode,'stdout_b64':base64.b64encode(result.stdout).decode(),'stderr_b64':base64.b64encode(result.stderr).decode()})
        if args.control=='exit' and not records:observations[0]['exit']=42
        byte_matches = observations[0] == observations[1]
        known933 = not byte_matches and hermes_login_divergence(observations[0], observations[1], mode, row['file_present'])
        known930 = not byte_matches and not known933 and reference_resolution_divergence(*observations)
        normalized = normalize_reference_resolution(observations[1])
        both = normalized is not None and hermes_login_divergence(observations[0], normalized, mode, row['file_present'])
        label = '#933 and #930: Hermes login diagnostic and unresolvable credential reference' if both else '#933: Hermes login missing/expired diagnostic' if known933 else '#930: unresolvable credential reference reported as configured' if known930 else None
        records.append({'id':row['id']+'-'+mode,'go':observations[0],'rust':observations[1],'matches':byte_matches or bool(label), 'byte_matches':byte_matches, **({'known_divergence':label} if label else {})})
if args.control=='missing-case':records.pop()
report={'schema_version':1,'cases':len(records),'expected_cases':104,'passed':sum(row['matches']for row in records),'failed':sum(not row['matches']for row in records),'binary_sha256':{name:hashlib.sha256(path.read_bytes()).hexdigest()for name,path in [('go',args.go_binary),('rust',args.rust_binary)]},'records':records}
report['byte_matches'] = sum(row['byte_matches'] for row in records)
report['known_divergences'] = sum('known_divergence' in row for row in records)
args.output.write_text(json.dumps(report,indent=2)+'\n', encoding='utf-8')
if len(records)!=104:sys.exit('Hermes CLI oracle: missing CLI case')
if report['failed']:sys.exit('Hermes CLI oracle: byte/exit mismatch: '+', '.join(row['id']for row in records if not row['matches']))
print(f"Hermes CLI oracle: {report['byte_matches']}/104 byte/exit matches; {report['known_divergences']} known #933/#930 divergences")
