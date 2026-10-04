"""Early argument classification versus frozen Go/reviewed parent and owned routing."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

ORIGINAL=[[b'--unknown'],[b'--unknown',b'--help'],[b'--help'],[b'--help',b'owned'],[b'--',b'--help'],[b'owned',b'--help'],[b'-h=false',b'owned'],[b'----help']]
EXTRA=[[b'----json'],[b'----unknown'],[b'---',b'--help'],[b'--=x'],[b'--help=anything'],[b'first',b'----help'],[b'--unknown=a=b'],[b'--',b'owned'],[b'--owned-\xff'],[b'--owned-\xe2\x82'],[b'owned-\xff'],[b'owned-\xe2\x82']]
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def state(home):
    values={}
    for p in home.rglob('*'):
        m=p.lstat();values[str(p.relative_to(home))]=dict(mode=m.st_mode,mtime_ns=m.st_mtime_ns,size=m.st_size,sha256=sha(p)if p.is_file()else None)
    return values
def process(binary,args,env,home):
    argv=[str(binary.resolve()),'usage',*[os.fsdecode(v)for v in args]]
    if binary.suffix=='.py':argv.insert(0,sys.executable)
    p=subprocess.run(argv,cwd=home,env=env,capture_output=True,timeout=15)
    return dict(exit=p.returncode,stdout_b64=base64.b64encode(p.stdout).decode(),stderr_b64=base64.b64encode(p.stderr).decode())
def main():
    parser=argparse.ArgumentParser()
    for key in ['go','parent','rust','output','sentinel']:parser.add_argument('--'+key,type=Path,required=True)
    args=parser.parse_args();rows=[];skipped=[];valid=[]
    with tempfile.TemporaryDirectory(prefix='usage-next-argv-')as directory:
        root=Path(directory)
        for variant in ['unsupported-base','ambiguous-copilot','unicode-kimi-device','overflow-nous-jwt']:
            home=root/variant;home.mkdir();env=dict(HOME=str(home),USERPROFILE=str(home),PATH='',XDG_CONFIG_HOME=str(home/'config'),XDG_DATA_HOME=str(home/'data'),XDG_CACHE_HOME=str(home/'cache'),ANTHROPIC_OAUTH_TOKEN='env://ABSENT')
            # Windows needs these process-runtime variables, never provider credentials.
            for key in ['SystemRoot','WINDIR','TEMP','TMP']:
                if key in os.environ:env[key]=os.environ[key]
            inputs={}
            if variant=='unsupported-base':env['KIMI_CODE_BASE_URL']='http://owned.invalid'
            if variant=='ambiguous-copilot':inputs['.config/github-copilot/apps.json']=b'{"github.com:a":{"oauth_token":"owned-a"},"github.com:b":{"oauth_token":"owned-b"}}'
            if variant=='unicode-kimi-device':inputs={'.kimi-code/credentials/kimi-code.json':b'{"access_token":"owned-token"}', '.kimi-code/device_id':'owned-雪'.encode()}
            if variant=='overflow-nous-jwt':inputs['.hermes/auth.json']=b'{"providers":[{"id":"nous","invoke_jwt":"header.eyJleHAiOjFlMTl9.signature"}]}'
            for name,data in inputs.items():p=home/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes(data)
            before=state(home)
            for mode in ['absent','present']:
                log=root/(variant+'-'+mode+'.jsonl');env.update(SYMBRAIN_GO_BINARY=str(root/'absent'if mode=='absent'else args.sentinel.resolve()),USAGE_NEXT_SENTINEL_LOG=str(log),USAGE_NEXT_SENTINEL_GO=str(args.go.resolve())if mode=='present'else '')
                for index,vector in enumerate(ORIGINAL+EXTRA):
                    if os.name=='nt':
                        try:[v.decode('utf8')for v in vector]
                        except UnicodeDecodeError:skipped.append(dict(variant=variant,mode=mode,argv_hex=[v.hex()for v in vector],reason='Windows argv cannot carry invalid Unix bytes'));continue
                    observed={};routed={}
                    for kind,binary in [('go',args.go),('parent',args.parent),('rust',args.rust)]:
                        before_log=log.read_bytes()if log.exists()else b''
                        observed[kind]=process(binary,vector,env,home)
                        after_log=log.read_bytes()if log.exists()else b''
                        routed[kind]=after_log[len(before_log):].decode()
                    assert not routed['go']and not routed['rust'],'native diagnostic never invokes fallback'
                    assert observed['go']['exit']==2 and observed['rust']==observed['go'],(variant,mode,index,observed)
                    # Parent's expected late admission invokes the owned sentinel on present Go.
                    parent_native_help=vector==[b'--help']
                    expected_parent=2 if parent_native_help or mode=='present'else 1
                    assert observed['parent']['exit']==expected_parent,(variant,mode,index,'retained parent boundary',observed)
                    calls=[json.loads(line)for line in log.read_text().splitlines()]if log.exists()else[]
                    if mode=='present'and not parent_native_help:assert calls[-1]==[v.hex()for v in [b'usage',*vector]],'parent preserves raw fallback argv'
                    assert state(home)==before
                    rows.append(dict(id=f'{variant}-{mode}-{index}',original=index<len(ORIGINAL),argv_hex=[v.hex()for v in vector],observed=observed,read_only=True,candidate_native=True,parent_fallback=mode=='present'and not parent_native_help,sentinel_calls=routed))
            if variant!='unicode-kimi-device':
                # Never send synthetic credentials to public endpoints. The owned
                # fallback only records argv and exits77 for valid report shapes.
                for index,vector in enumerate([[],[b'--'],[b'---'],[b'--json'],[b'--output',b'json']]):
                    log=root/f'{variant}-report-{index}.jsonl';env.update(SYMBRAIN_GO_BINARY=str(args.sentinel.resolve()),USAGE_NEXT_SENTINEL_LOG=str(log),USAGE_NEXT_SENTINEL_GO='')
                    value=process(args.rust,vector,env,home);assert value['exit']==77
                    calls=[json.loads(line)for line in log.read_text().splitlines()];assert calls==[[v.hex()for v in [b'usage',*vector]]]
                    assert state(home)==before;valid.append(dict(id=f'{variant}-valid-{index}',argv_hex=[v.hex()for v in vector],observed=value,sentinel_calls=calls,read_only=True))
        assert sum(r['original']for r in rows)==64
    args.output.write_text(json.dumps(dict(cases=len(rows),original_cases=64,passed=len(rows),failed=0,valid_gated_reports=valid,skipped=skipped,records=rows,binaries={str(p.resolve()):sha(p)for p in [args.go,args.parent,args.rust,args.sentinel]},scope='All original64 inherited admission inputs retained, candidate native exactGo errors/help/positionals regardless fallback presence, parent boundary retained; valid gated report argv byte-preserving owned sentinel, no public HTTP'),indent=2)+'\n')
    print('PASS early argv',len(rows),'including original64; valid fallback',len(valid))
if __name__=='__main__':main()
