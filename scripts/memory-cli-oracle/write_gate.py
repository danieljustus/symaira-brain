#!/usr/bin/env python3
"""Build frozen Go in an owned root, replay native writes and actual mutants."""
import argparse
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import deletes
import replay
import writes
import write_boundaries


def main():
    parser=argparse.ArgumentParser(); parser.add_argument('--go',type=Path); parser.add_argument('--rust',type=Path); parser.add_argument('--report-dir',type=Path,required=True)
    args=parser.parse_args(); repo=Path(__file__).resolve().parents[2]
    rust=(args.rust or repo/'target/debug'/('symbrain.exe' if os.name=='nt' else 'symbrain')).resolve()
    reports=args.report_dir.resolve(); reports.mkdir(parents=True,exist_ok=True)
    archive=replay.run(['git','archive',replay.ORACLE],repo).stdout
    caches=replay.run(['go','env','GOMODCACHE','GOCACHE'],repo).stdout.decode().splitlines()
    with tempfile.TemporaryDirectory(prefix='memory-write-oracle-') as temporary:
        root=Path(temporary); source=root/'source'; source.mkdir(); home=root/'home'; home.mkdir()
        with tarfile.open(fileobj=io.BytesIO(archive)) as files: files.extractall(source,filter='data')
        env=replay.isolated_env(home); env.update(PATH=os.environ.get('PATH',''),GOWORK='off',GOENV='off',GOMODCACHE=caches[0],GOCACHE=caches[1],CGO_ENABLED='0')
        go=args.go.resolve() if args.go else root/('oracle.exe' if os.name=='nt' else 'oracle')
        if args.go is None: replay.run(['go','build','-o',str(go),'./cmd/symbrain'],source,env)
        control_source=source/'scripts/memory-write-control/main.go'; control_source.parent.mkdir(parents=True)
        control_source.write_bytes((repo/'scripts/memory-cli-oracle/write_control.go.txt').read_bytes())
        wrapper=root/('control.exe' if os.name=='nt' else 'control')
        replay.run(['go','build','-o',str(wrapper),'./scripts/memory-write-control'],source,env)
        fallback_source=source/'scripts/memory-write-fallback/main.go'; fallback_source.parent.mkdir(parents=True)
        fallback_source.write_bytes((repo/'scripts/memory-cli-oracle/write_fallback.go.txt').read_bytes())
        fallback=root/('fallback.exe' if os.name=='nt' else 'fallback')
        replay.run(['go','build','-o',str(fallback),'./scripts/memory-write-fallback'],source,env)
        writes.execute(go,rust,reports/'writes.json')
        deletes.execute(go,rust,reports/'deletes.json')
        write_boundaries.execute(go,rust,fallback,reports/'fallback-boundaries.json')
        controls=[]
        expected={'identity':'one returned primary row','audit':'actual Go/native write state differs','exit':'native set exit must be zero'}
        for mode in ('identity','audit','exit'):
            receipt=reports/(mode+'-failure.json')
            command=[sys.executable,str(repo/'scripts/memory-cli-oracle/writes.py'),'--go',str(go),'--rust',str(wrapper),'--report',str(receipt)]
            controlled_env=dict(env,MEMORY_CLI_CONTROL_RUST=str(rust),MEMORY_CLI_CONTROL_MODE=mode)
            result=subprocess.run(command,cwd=repo,env=controlled_env,capture_output=True,timeout=180)
            (reports/(mode+'-failure.stdout')).write_bytes(result.stdout)
            (reports/(mode+'-failure.stderr')).write_bytes(result.stderr)
            detected=result.returncode!=0 and expected[mode].encode() in result.stderr and receipt.is_file()
            controls.append(dict(mode=mode,exit=result.returncode,detected=detected,stdout_sha256=replay.digest(result.stdout),stderr_sha256=replay.digest(result.stderr),report_sha256=replay.digest(receipt.read_bytes()) if receipt.is_file() else None))
            assert detected, (mode,result.returncode,result.stderr)
        files=replay.run(['git','ls-files','--cached','--others','--exclude-standard','rust/symbrain-cli','rust/symbrain-memory','scripts/memory-cli-oracle','.github/workflows/memory-cli-native.yml','Cargo.toml','Cargo.lock'],repo).stdout.decode().splitlines()
        receipt=dict(candidate_revision=replay.run(['git','rev-parse','HEAD'],repo).stdout.decode().strip(),candidate_dirty=bool(replay.run(['git','status','--porcelain'],repo).stdout),go_revision=replay.ORACLE,
                     go_binary_sha256=replay.digest(go.read_bytes()),rust_binary_sha256=replay.digest(rust.read_bytes()),control_binary_sha256=replay.digest(wrapper.read_bytes()),fallback_binary_sha256=replay.digest(fallback.read_bytes()),go_version=replay.run(['go','version'],repo).stdout.decode().strip(),
                     candidate_source_sha256={name:replay.digest((repo/name).read_bytes()) for name in sorted(set(files))},
                     frozen_go_source_sha256={str(path.relative_to(source)):replay.digest(path.read_bytes()) for path in sorted(source.rglob('*.go')) if not str(path.relative_to(source)).startswith(('scripts/memory-write-control/','scripts/memory-write-fallback/'))},
                     controls=controls,operator_home_used=False,embedding_endpoints_owned=True,paid_endpoints_used=False,
                     report_sha256={path.name:replay.digest(path.read_bytes()) for path in sorted(reports.iterdir()) if path.is_file() and path.name!='receipt.json'})
        (reports/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')


if __name__=='__main__': main()
