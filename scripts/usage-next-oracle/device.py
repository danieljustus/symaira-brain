"""Fresh frozen-Go constructors plus hostname-verified owned TLS/native proof."""
import argparse
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import subprocess
import tempfile
from peer import Peer
from cli_build import build

FROZEN='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def main():
    parser=argparse.ArgumentParser();parser.add_argument('output',type=Path);args=parser.parse_args();output=args.output.resolve();output.parent.mkdir(parents=True,exist_ok=True)
    repo=Path(__file__).resolve().parents[2];evidence=output.with_suffix('.evidence');evidence.mkdir(exist_ok=True)
    def run(label,command,env=None,cwd=None):
        process=subprocess.run(command,cwd=cwd or repo,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=300)
        (evidence/(label+'.log')).write_bytes(process.stdout)
        assert process.returncode==0,(label,process.returncode,process.stdout[-8000:])
    with tempfile.TemporaryDirectory(prefix='usage-next-owned-')as directory:
        root=Path(directory);source=root/'go-source';peer=None
        run('input',['python3',str(repo/'scripts/usage-next-oracle/device_cases.py'),str(evidence/'input.json')])
        cases=json.loads((evidence/'input.json').read_text());assert len(cases)==64
        original=json.loads((repo/'migration/evidence/usage-remaining-baseline-768/symaira-usage768-remaining-device-input.json').read_text());assert cases[:39]==original and len(original)==39
        run('status-input',['python3',str(repo/'scripts/usage-next-oracle/status_cases.py'),str(evidence/'status-input.json')])
        statuses=json.loads((evidence/'status-input.json').read_text())
        run('source',['git','worktree','add','--quiet','--detach',str(source),FROZEN])
        try:
            shutil.copyfile(repo/'scripts/usage-next-oracle/device_test.go.txt',source/'internal/usage/device_next_768_test.go')
            shutil.copyfile(repo/'scripts/usage-next-oracle/status_test.go.txt',source/'internal/usage/status_next_768_test.go')
            shutil.copyfile(repo/'scripts/usage-next-oracle/certificate.go.txt',root/'certificate.go')
            for name in ['api','web']:shutil.copyfile(source/f'internal/usage/testdata/kimi-{name}-usages.json',evidence/f'{name}.json')
            env=os.environ.copy();env.update(GOTOOLCHAIN='go1.26.7',CGO_ENABLED='0',USAGE_REMAINING_INPUT=str(evidence/'input.json'),USAGE_REMAINING_ROOT=str(root/'homes'),USAGE_REMAINING_OUTPUT=str(evidence/'go.json'))
            run('certificate',['go','run',str(root/'certificate.go'),str(root)],env)
            # Retain only the public trust certificates, never an operator secret.
            shutil.copyfile(root/'ca.pem',evidence/'ca.pem');shutil.copyfile(root/'leaf.pem',evidence/'leaf.pem')
            suffix='.exe'if os.name=='nt'else''
            go_tests=evidence/('go-provider-tests'+suffix)
            run('go-tests-build',['go','-C',str(source),'test','-c','-trimpath','-o',str(go_tests),'./internal/usage'],env)
            run('go-canned',[str(go_tests),'-test.run','^TestRemainingDeviceBaseline768$','-test.count=1'],env,source/'internal/usage')
            bounds=[dict(id=f'transport-bound-{status}-{kind}',responses=[status],kind='transport-bound',length=1048576+(kind=='over'))for status in [200,401]for kind in ['exact','over']]
            peer=Peer(root,cases+statuses+bounds,(evidence/'api.json').read_bytes(),(evidence/'web.json').read_bytes())
            peer.fixtures={row['fixture']:(source/'internal/usage/testdata'/row['fixture']).read_bytes()for row in statuses}
            env.update(USAGE_DEVICE_PEER=peer.address,USAGE_DEVICE_CA=str(root/'ca.pem'),USAGE_REMAINING_OUTPUT=str(evidence/'go-wire.json'))
            run('go-tls',[str(go_tests),'-test.run','^TestRemainingDeviceBaseline768$','-test.count=1'],env,source/'internal/usage')
            env.update(USAGE_STATUS_ROOT=str(root/'status-homes'),USAGE_STATUS_INPUT=str(evidence/'status-input.json'),USAGE_STATUS_GO=str(evidence/'status-go.json'),USAGE_STATUS_HOSTS='|api.kimi.com||www.kimi.com||api.anthropic.com||chatgpt.com||api.github.com||cursor.com||api.moonshot.ai||portal.nousresearch.com||opencode.ai||openrouter.ai|')
            run('go-status-tls',[str(go_tests),'-test.run','^TestUsageStatusTLS768$','-test.count=1'],env,source/'internal/usage')
            peer.phase='rust';env.update(USAGE_DEVICE_GO=str(evidence/'go.json'),USAGE_DEVICE_WIRE_GO=str(evidence/'go-wire.json'),USAGE_DEVICE_API=str(evidence/'api.json'),USAGE_DEVICE_WEB=str(evidence/'web.json'),USAGE_DEVICE_NATIVE=str(evidence/'native.json'),USAGE_STATUS_NATIVE=str(evidence/'status-native.json'))
            run('native',['cargo','test','--locked','-p','symbrain-usage','--lib','device_oracle_matches_fresh_go','--','--ignored','--nocapture'],env)
            matches=re.findall(r'Running unittests [^\n]*\(([^)]+)\)',(evidence/'native.log').read_text());assert len(matches)==1
            native_test_path=Path(matches[0]);native_test_path=native_test_path if native_test_path.is_absolute()else repo/native_test_path
            target=Path(env.get('CARGO_TARGET_DIR',repo/'target')).resolve();assert native_test_path.resolve().is_relative_to(target)
            native_tests=evidence/('native-provider-tests'+suffix);shutil.copy2(native_test_path,native_tests)
            provider_binaries={kind:dict(path=str(path),bytes=path.stat().st_size,sha256=sha(path))for kind,path in [('go',go_tests),('native',native_tests)]}
            main_wire=list(peer.rows)
            controls=[]
            for kind,field,message in [('missing-device-case','go.json','left: 63'),('device-header-bytes','go.json','complete request/raw header bytes'),('remote-status-diagnostic','status-go.json','actual remote full status/report')]:
                path=evidence/field;original=path.read_bytes();rows=json.loads(original)
                if kind=='missing-device-case':rows.pop()
                elif kind=='device-header-bytes':rows[0]['requests'][0]['header_value_hex']['X-Msh-Device-Id']=['00']
                else:rows[0]['report']['providers'][0]['error']='owned incorrect401 diagnostic'
                path.write_text(json.dumps(rows));peer.phase='control-'+kind
                try:
                    command=['cargo','test','--locked','-p','symbrain-usage','--lib','device_oracle_matches_fresh_go','--','--ignored','--nocapture']
                    process=subprocess.run(command,cwd=repo,env=env,capture_output=True,timeout=120)
                    raw=process.stdout+process.stderr;(evidence/(kind+'.log')).write_bytes(raw)
                    assert process.returncode==101 and message.encode()in raw and b'1 failed'in raw,(kind,process.returncode,raw[-8000:])
                    controls.append(dict(id=kind,exit=process.returncode,intended_assertion=message,rejected=True,mutated_input_sha256=sha(path)))
                finally:path.write_bytes(original)
            (evidence/'controls.json').write_text(json.dumps(controls,indent=2)+'\n')
            peer.close();(evidence/'wire.json').write_text(json.dumps(main_wire,indent=2)+'\n');(evidence/'controls-wire.json').write_text(json.dumps(peer.rows[len(main_wire):],indent=2)+'\n');peer=None
            go=json.loads((evidence/'go-wire.json').read_text());native=json.loads((evidence/'native.json').read_text());wire=json.loads((evidence/'wire.json').read_text());assert len(go)==native['wire']['cases']==57
            observations=[]
            statuses_go=json.loads((evidence/'status-go.json').read_text());assert native['statuses']['cases']==len(statuses_go)==84
            for row in go+statuses_go:
                for index,request in enumerate(row['requests']):
                    actual={phase:next(r for r in wire if r['phase']==phase and r['id']==row['id']and r['index']==index)for phase in ['go','rust']}
                    for phase,data in actual.items():
                        for name,values in request['header_value_hex'].items():
                            if name.lower()=='x-server-instance' and phase=='rust':
                                assert data['headers_hex'][name.lower()]==['server-fn:00000000-0000-0000-0000-000000000000'.encode().hex()]
                            else:assert data['headers_hex'][name.lower()]==values,(phase,row['id'],name)
                        assert data['body_hex']==request['body_hex']
                    assert actual['go']['request_line_hex']==actual['rust']['request_line_hex'] and actual['go']['response_hex']==actual['rust']['response_hex']
                    defaults={phase:{name:values for name,values in actual[phase]['headers_hex'].items()if name not in {k.lower()for k in request['headers']} and not name.startswith('x-owned-')}for phase in ['go','rust']}
                    observations.append(dict(id=row['id'],index=index,provider_headers_raw_equal_except_inherited_opencode_instance=row.get('provider')=='opencode',provider_headers_raw_equal=row.get('provider')!='opencode',body_requestline_response_equal=True,transport_default_headers=defaults,full_request_byte_equal=actual['go']['request_hex']==actual['rust']['request_hex']))
            assert native['bounds']['cases']==4
            assert len(wire)==2*len(observations)+4 and len({(r['phase'],r['id'],r['index'])for r in wire})==len(wire)
            cli_build=build(repo,root,evidence,source,env,run)
            argv_command=['python3',str(repo/'scripts/usage-next-oracle/argv.py')]
            for key in ['go','parent','rust','sentinel']:argv_command+=['--'+key,cli_build['binaries'][key]['path']]
            run('early-argv',argv_command+['--output',str(evidence/'argv.json')],env)
            control_command=['python3',str(repo/'scripts/usage-next-oracle/argv_controls.py')]+argv_command[2:]
            run('early-argv-controls',control_command+['--output',str(evidence/'argv-controls.json')],env)
            argv=json.loads((evidence/'argv.json').read_text());argv_controls=json.loads((evidence/'argv-controls.json').read_text())
            assert argv['original_cases']==64 and argv['failed']==0 and len(argv['valid_gated_reports'])==15 and argv_controls['rejected']==2
            assert not subprocess.check_output(['git','-C',str(source),'diff','--name-only'])
            frozen={}
            for p in sorted((source/'internal/usage').glob('*.go'))+[source/'go.mod',source/'go.sum']:
                if p.name in ['device_next_768_test.go','status_next_768_test.go']:continue
                relative=p.relative_to(source).as_posix();assert p.read_bytes()==subprocess.check_output(['git','-C',str(source),'show',FROZEN+':'+relative]);frozen[relative]=sha(p)
            head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip();dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=repo))
            paths=sorted((repo/'rust/symbrain-usage/src').rglob('*.rs'))+sorted((repo/'scripts/usage-next-oracle').glob('*'))+[repo/p for p in ['Cargo.lock','rust/symbrain-usage/Cargo.toml','rust/symbrain-cli/src/usage_cli.rs','rust/symbrain-cli/src/lib.rs']]
            paths=[p for p in paths if p.is_file()];source_hashes={str(p.relative_to(repo)):sha(p)for p in paths}
            if not dirty:
                for p in paths:assert p.read_bytes()==subprocess.check_output(['git','show',head+':'+p.relative_to(repo).as_posix()],cwd=repo)
            receipt=dict(candidate_head=head,candidate_dirty=dirty,oracle_commit=FROZEN,frozen_source_sha256=frozen,source_sha256=source_hashes,cases=64,original_device_cases=39,exhaustive_trim_cases=25,full_native_reports=57,retained_gates=7,remote_status_strategy_cases=84,provider_test_binaries=provider_binaries,negative_controls=controls,argv=argv,argv_controls=argv_controls,cli_build=cli_build,native=native,wire_observations=observations,evidence_sha256={str(p.name):sha(p)for p in evidence.iterdir()if p.is_file()},go_sdk=subprocess.check_output(['go','version'],text=True).strip(),rust_sdk=subprocess.check_output(['rustc','-Vv'],text=True),native_platforms='Linux author proof only; fresh native macOS/Windows required',clock_policy='All actual Go/native snapshot fetched_at retained and bounded by invocation; canonicalization uses the invocation clock plus exact OpenCode3600/86400 second reset offsets relative to actual fetched_at. No fixed/runtime equality claim.',wire_policy='Exact provider-added header bytes except inherited OpenCode X-Server-Instance runtime/fixed identity, request line/body and response compared. All full raw wire requests retained; inherited transport default/header serialization differences are explicit, not exact wire parity.',private_seam='Test-only trust root and resolver/dial pin original HTTPS public hostnames to owned loopback TLS peer, certificate hostname verified; no provider/operator network. Production execute/parser unchanged. Fast TLS proof does not establish deadline/cancel parity.')
            output.write_text(json.dumps(receipt,indent=2)+'\n');print('PASS64/57 full native,7 gated; owned TLS comparisons',len(observations))
        finally:
            if peer:
                (evidence/'wire-failed.json').write_text(json.dumps(dict(rows=peer.rows,errors=peer.errors),indent=2)+'\n');peer.close()
            subprocess.run(['git','-C',str(repo),'worktree','remove','--force',str(source)],check=True)
if __name__=='__main__':main()
