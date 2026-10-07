"""Require real failed native replays for absent, corrupt or incomplete inputs."""
import json
import os
import pathlib
import subprocess
import sys

scratch=pathlib.Path(sys.argv[1])
rust=pathlib.Path(sys.argv[2])
go=scratch/('go-usage.exe'if os.name=='nt'else'go-usage')
cli=pathlib.Path(__file__).with_name('cli.py')
controls=[]
for control,diagnostic in [('exit','byte/exit mismatch'),('missing-case','missing CLI case')]:
    result=subprocess.run([sys.executable,str(cli),str(scratch/'go.json'),str(go),str(rust),str(scratch/('cli-control-'+control+'.json')),'--control',control],capture_output=True,check=False,timeout=120)
    assert result.returncode!=0 and diagnostic.encode()in result.stderr,(control,result.stderr.decode())
    controls.append({'id':'cli-'+control,'exit':result.returncode,'intended_diagnostic':diagnostic,'stderr':result.stderr.decode()})
fixture=scratch/'go.json'
original=fixture.read_bytes()
for control,diagnostic in [('missing-oracle','Go Copilot/Kimi evidence'),('mutated-report','complete Copilot/Kimi report'),('duplicate-case','exact Copilot/Kimi case coverage')]:
    try:
        if control=='missing-oracle':fixture.unlink()
        else:
            records=json.loads(original)
            if control=='duplicate-case':records[0]['id']=records[1]['id']
            else:records[0]['report']['providers'][0]['auth_status']['detail']='intentional-provider-files-control'
            fixture.write_text(json.dumps(records), encoding='utf-8')
        result=subprocess.run(['cargo','test','--locked','-p','symbrain-usage','--lib','copilot_kimi_oracle_matches_fresh_go','--','--ignored','--nocapture'],capture_output=True,check=False,timeout=120)
        output=result.stdout+result.stderr
        assert result.returncode==101 and diagnostic.encode()in output and b'1 failed'in output,(control,output.decode())
        controls.append({'id':control,'exit':result.returncode,'intended_diagnostic':diagnostic,'output':output.decode()})
    finally:fixture.write_bytes(original)
assert len(controls)==5
(scratch/'controls.json').write_text(json.dumps(controls,indent=2)+'\n', encoding='utf-8')

# Wrong-owner requests must fail the real production-constructor byte assertion.
owner=scratch/'owner.json'
original=owner.read_bytes()
try:
    rows=json.loads(original)
    rows[0]['requests'][0]['headers']['Authorization']=['Bearer wrong-owner-control']
    rows[0]['requests'][0]['header_value_hex']['Authorization']=['Bearer wrong-owner-control'.encode().hex()]
    owner.write_text(json.dumps(rows), encoding='utf-8')
    result=subprocess.run(['cargo','test','--locked','-p','symbrain-usage','--lib','copilot_kimi_oracle_matches_fresh_go','--','--ignored','--nocapture'],capture_output=True,check=False,timeout=120)
    output=result.stdout+result.stderr
    assert result.returncode==101 and b'complete owner request bytes'in output and b'1 failed'in output,output.decode()
    (scratch/'owner-controls.json').write_text(json.dumps([dict(id='wrong-owner-request',exit=result.returncode,intended_diagnostic='complete owner request bytes',output=output.decode())],indent=2)+'\n', encoding='utf-8')
finally:owner.write_bytes(original)
