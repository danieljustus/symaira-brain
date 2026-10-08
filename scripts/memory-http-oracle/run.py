#!/usr/bin/env python3
"""Native-platform frozen Go build and full private HTTP/DOM/failure gates."""
import argparse
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import support


def invoke(command, cwd, env, log):
    result=subprocess.run(command,cwd=cwd,env=env,capture_output=True)
    log.write_bytes(result.stdout+result.stderr)
    return result


def run(args):
    repo=Path(__file__).resolve().parents[2];root=args.report_dir.resolve();root.mkdir(exist_ok=False)
    source=root/'frozen-go';source.mkdir();home=root/'build-home';home.mkdir()
    archive=subprocess.check_output(['git','archive',support.ORACLE],cwd=repo)
    with tarfile.open(fileobj=io.BytesIO(archive)) as files:files.extractall(source,filter='data')
    version=subprocess.check_output(['go','version'],text=True).strip()
    assert 'go1.26.7 ' in version,version
    caches=subprocess.check_output(['go','env','GOMODCACHE','GOCACHE'],text=True).splitlines()
    env=dict(os.environ,HOME=str(home),USERPROFILE=str(home),GOWORK='off',GOENV='off',CGO_ENABLED='0',GOMODCACHE=caches[0],GOCACHE=caches[1])
    for key in ['GOOS','GOARCH','JWT_SECRET_KEY']:
        env.pop(key,None)
    go=root/('frozen-go.exe' if os.name=='nt' else 'frozen-go-cli')
    result=invoke(['go','build','-o',str(go),'./cmd/symbrain'],source,env,root/'go-build.log')
    assert result.returncode==0,(root/'go-build.log').read_text()
    # Verify source before passing the newly built actual platform executable.
    manifest={p.relative_to(source).as_posix():support.digest(p) for p in sorted(source.rglob('*')) if p.is_file()}
    env=dict(os.environ,MEMORY_HTTP_ORACLE_GO=str(go),MEMORY_HTTP_ORACLE_SHA=support.digest(go),MEMORY_HTTP_ORACLE_SOURCE=str(source))
    records=[]
    for mode in [None,'wrong-secret','empty-state']:
        label=mode or 'http';report=root/(label+'.json')
        command=[sys.executable,str(repo/'scripts/memory-http-oracle/replay.py'),'--native',str(args.native.resolve()),'--report',str(report)]
        if mode:command+=['--control',mode]
        result=invoke(command,repo,env,root/(label+'.log'));proof=json.loads(report.read_text())
        if mode:
            last=proof['records'][-1]
            assert result.returncode==1 and last['id']=='cors-extension' and not last['match'],(mode,result.returncode,last)
        else:assert result.returncode==0,(root/(label+'.log')).read_text()
        records.append(dict(mode=mode,exit=result.returncode,report_sha256=support.digest(report),native_sha256=proof['native_sha256'],go_sha256=proof['go_sha256']))
    for mode in [None,'omit-duplicate','foreign-to-local']:
        label='authority-'+(mode or 'http');report=root/(label+'.json')
        command=[sys.executable,str(repo/'scripts/memory-http-oracle/authority.py'),'--native',str(args.native.resolve()),'--report',str(report)]
        if mode:command+=['--control',mode]
        result=invoke(command,repo,env,root/(label+'.log'));proof=json.loads(report.read_text())
        if mode:
            expected='duplicate-host-local-first-read' if mode=='omit-duplicate' else 'absolute-foreign-target-read'
            assert result.returncode==1 and proof['records'][-1]['name']==expected and not proof['records'][-1]['match'],(mode,result.returncode)
        else:assert result.returncode==0 and len(proof['records'])==15,(root/(label+'.log')).read_text()
        records.append(dict(mode=label,exit=result.returncode,report_sha256=support.digest(report),native_sha256=proof['native_sha256'],go_sha256=proof['go_sha256']))
    command=[sys.executable,str(repo/'scripts/memory-http-oracle/transport.py'),'--native',str(args.native.resolve()),'--report',str(root/'transport.json')]
    result=invoke(command,repo,env,root/'transport.log');assert result.returncode==0,(root/'transport.log').read_text()
    command=[sys.executable,str(repo/'scripts/memory-http-oracle/same_origin.py'),'--native',str(args.native.resolve()),'--report',str(root/'same-origin.json')]
    result=invoke(command,repo,env,root/'same-origin.log');assert result.returncode==0,(root/'same-origin.log').read_text()
    command=[sys.executable,str(repo/'scripts/memory-http-oracle/dom.py'),'--native',str(args.native.resolve()),'--node',str(args.node.resolve()),'--jsdom',str(args.jsdom.resolve()),'--report',str(root/'dom.json')]
    result=invoke(command,repo,env,root/'dom.log');assert result.returncode==0,(root/'dom.log').read_text()
    node_version=subprocess.check_output([str(args.node),'--version'],text=True).strip()
    module=json.loads((args.jsdom/'package.json').read_text());assert module['version']=='26.1.0',module['version']
    receipt=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo)),platform=platform.platform(),go_version=version,node_version=node_version,jsdom_version=module['version'],frozen_go_revision=support.ORACLE,frozen_source_sha256=manifest,go_binary_sha256=support.digest(go),native_binary_sha256=support.digest(args.native),records=records)
    (root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(dict(head=receipt['head'],platform=receipt['platform'],gates=9,controls=4,report_dir=str(root))))

if __name__=='__main__':
    parser=argparse.ArgumentParser();parser.add_argument('--native',type=Path,required=True);parser.add_argument('--node',type=Path,required=True);parser.add_argument('--jsdom',type=Path,required=True);parser.add_argument('--report-dir',type=Path,required=True);run(parser.parse_args())
