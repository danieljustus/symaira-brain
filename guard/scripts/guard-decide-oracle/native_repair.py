#!/usr/bin/env python3
"""Native CI: build immutable Go oracle and candidate; execute F01-F05 gates."""
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import hashlib

ROOT = Path(__file__).resolve().parents[3]
PIN = '0b585d52915a824664e1377d0a995dff3f5405cd'
OUT = Path(os.environ['GUARD_REPAIR_OUTPUT']).resolve()
OUT.mkdir(parents=True, exist_ok=True)


def main():
    head = subprocess.check_output(['git','rev-parse','HEAD'], cwd=ROOT, text=True).strip()
    env = dict(os.environ)
    env['CARGO_HOME'] = os.environ.get('CARGO_HOME', str(Path.home()/'.cargo'))
    env['RUSTUP_HOME'] = os.environ.get('RUSTUP_HOME', str(Path.home()/'.rustup'))
    # Build tools retain installation/toolchain roots; all writable application
    # and Go build caches are isolated. Runtime probes use a narrower allowlist.
    for key,name in [('HOME','home'),('USERPROFILE','profile'),('XDG_CONFIG_HOME','config'),('XDG_DATA_HOME','data'),('XDG_CACHE_HOME','cache'),('GOCACHE','gocache'),('TMP','tmp'),('TEMP','tmp'),('TMPDIR','tmp'),('CARGO_TARGET_DIR','target')]:
        path=OUT/name; path.mkdir(exist_ok=True); env[key]=str(path)
    env.update(GOTOOLCHAIN='go1.26.7', CARGO_BUILD_JOBS='2')
    env['CGO_ENABLED']='0'
    suffix='.exe' if os.name=='nt' else ''
    rows=[]
    def run(name,cmd,cwd=ROOT):
        with (OUT/(name+'.log')).open('wb') as log:
            result=subprocess.run(cmd,cwd=cwd,env=env,stdout=log,stderr=subprocess.STDOUT,timeout=900)
        rows.append(dict(name=name,argv=cmd,exit=result.returncode))
        (OUT/'commands.json').write_text(json.dumps(dict(head=head,commands=rows),indent=2))
        print(name,result.returncode,flush=True)
        if result.returncode: raise RuntimeError((OUT/(name+'.log')).read_text(errors='replace')[-6000:])
    meta=json.loads(subprocess.check_output(['cargo','metadata','--manifest-path',str(ROOT/'Cargo.toml'),'--no-deps','--format-version','1'],cwd=ROOT,env=env))
    assert Path(meta['workspace_root']).resolve()==ROOT
    # Source archive is a trusted immutable Git object, not ambient checkout Go.
    archive=subprocess.check_output(['git','archive',PIN],cwd=ROOT)
    with tempfile.TemporaryDirectory(prefix='guard-go-',dir=OUT) as temp:
        source=Path(temp)
        with tarfile.open(fileobj=io.BytesIO(archive)) as tar: tar.extractall(source,filter='data')
        go=OUT/('symbrain-go'+suffix)
        run('go-build',['go','build','-o',str(go),'./cmd/symbrain'],source)
        run('rust-build',['cargo','build','--manifest-path',str(ROOT/'Cargo.toml'),'--locked','-p','symbrain-cli'])
        rust=OUT/'target/debug'/('symbrain'+suffix)
        run('differential',['python',str(ROOT/'guard/scripts/guard-decide-oracle/repair_parity.py'),'--go',str(go),'--rust',str(rust),'--report',str(OUT/'differential.json')])
        run('regressions',['cargo','test','--manifest-path',str(ROOT/'Cargo.toml'),'--locked','-p','symbrain-guard-core','--test','external_repair'])
        run('adapter',['cargo','test','--manifest-path',str(ROOT/'Cargo.toml'),'--locked','-p','symbrain-cli','--test','guard_decide_adapter'])
        (OUT/'identity.json').write_text(json.dumps(dict(head=head,oracle_commit=PIN,binary_sha256={str(p.name):hashlib.sha256(p.read_bytes()).hexdigest() for p in [go,rust]}),indent=2))

if __name__=='__main__': main()
