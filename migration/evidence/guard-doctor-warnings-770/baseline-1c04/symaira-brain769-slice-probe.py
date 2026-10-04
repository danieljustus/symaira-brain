#!/usr/bin/env python3
import base64, hashlib, json, os, pathlib, shutil, subprocess, tempfile

ROOT=pathlib.Path('/workspace/symaira-guard770-doctor-boundaries')
OUT=pathlib.Path('/tmp/symaira-brain769-positive-slice-probe.json')
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0')
OLD=pathlib.Path('/workspace/oracles/symaira-guard770-e867c2bb-binaries/symbrain')
def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def result(p): return {'exit':p.returncode, 'stdout_b64':base64.b64encode(p.stdout).decode(), 'stderr_b64':base64.b64encode(p.stderr).decode()}
def invoke(exe,args,env,cwd): return subprocess.run([str(exe),*args], input=b'',stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,cwd=cwd,timeout=15)
cases=[
('unknown-root','owned=1\n'),
('unknown-root-array','owned=[{nested=1},{nested=2}]\n'),
('unknown-root-table','[owned]\nnested=1\n'),
('unknown-quoted-dot','"owned.dot"=1\n'),
('unknown-unicode','"owned\\u0085"=1\n'),
('unknown-proxy','[proxy]\nowned=1\n'),
('unknown-audit','[audit]\nowned=1\n'),
('unknown-sequence','[sequence]\nowned=1\n'),
('unknown-spawn','[spawn]\nowned=1\n'),
('unknown-rule','[[rules]]\ndecision="allow"\nowned=1\n[rules.match]\nserver="owned"\n'),
('unknown-match','rules=[{decision="allow",match={server="owned",owned=1}}]\n'),
('unknown-remote','remote=[{name="owned",extra=1}]\n'),
('unknown-allowlist','spawn={allowlist=[{path="/owned/synthetic",extra=1}]}\n'),
('unknown-all-order','z=1\n[audit]\nlast=1\nfirst=2\n[proxy]\nunknown=3\n'),
('defaults-dynamic','[defaults]\nowned="allow"\n'),
('warning-then-semantic-error','owned=1\n[sequence]\nenabled=true\nthreshold=1\n'),
('type-error-suppresses-warning','owned=1\n[sequence]\nenabled="bad"\n'),
]
report={'source_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),'source_commit':'9603d24e6749ee67e5eac1b7ddda6900af022cd5','mode':'readonly recommendations; actual whole frozen Go; archived native only historical routing observations','binaries':{'go':{'path':str(GO),'sha256':sha(GO)},'historical_native':{'path':str(OLD),'sha256':sha(OLD),'source':'e867c2bb88b9931f83f93ad7e880b84c9a0260f8'}},'cases':[]}
with tempfile.TemporaryDirectory(prefix='symaira-brain769-positive-') as td:
    t=pathlib.Path(td); shutil.copyfile(GO,t/'go'); (t/'go').chmod(0o700); shutil.copyfile(OLD,t/'native'); (t/'native').chmod(0o700)
    home=t/'home'; home.mkdir(); config=t/'config'; config.mkdir(); data=t/'data'; data.mkdir(); cache=t/'cache'; cache.mkdir(); work=t/'work'; work.mkdir()
    sentinel=t/'fallback'; sentinel.write_text('#!/bin/sh\nprintf "fallback invoked\\n" >&2\nexit 93\n');sentinel.chmod(0o700)
    env={'HOME':str(home),'XDG_CONFIG_HOME':str(config),'XDG_DATA_HOME':str(data),'XDG_CACHE_HOME':str(cache),'PATH':'','TZ':'UTC','LANG':'C','SYMBRAIN_GO_BINARY':str(sentinel),'SYMGUARD_CONFIG':str(config/'guard.toml')}
    for name,text in cases:
        (config/'guard.toml').write_bytes(text.encode()); p=invoke(t/'go',['guard','doctor'],env,work)
        report['cases'].append({'id':name,'argv':['guard','doctor'],'input_utf8':text,'go':result(p),'warning_lines':p.stderr.decode(errors='backslashreplace').splitlines()})
    text=dict(cases)['unknown-all-order']; (config/'guard.toml').write_bytes(text.encode()); repeat=[result(invoke(t/'go',['guard','doctor'],env,work)) for _ in range(10)]
    report['unknown_warning_repeat']={'input_utf8':text,'runs':repeat,'distinct_stderr':len({p['stderr_b64'] for p in repeat})}
    (config/'guard.toml').unlink()
    for args in [['config','--help'],['config','-h'],['config','-unknown'],['config','-'],['config','--','path'],['config','path'],['config','get'],['audit','tail','--json'],['audit','tail','-n','0x2'],['install','--harness','claude','--profile','owned','--dry-run'],['uninstall','--harness','claude','--dry-run'],['mcp'],['guard','help'],['guard','scan','--format','json'],['guard','grants','list']]:
        go=invoke(t/'go',args,env,work); native=invoke(t/'native',args,env,work)
        report['cases'].append({'id':'routing-'+ '-'.join(args),'argv':args,'go':result(go),'historical_native':result(native),'historical_fallback_invoked':native.returncode==93})
report['source_files']={str(p.relative_to(ROOT)):sha(p) for p in [ROOT/'rust/symbrain-cli/src/lib.rs',*[ROOT/'rust/symbrain-cli/src'/f'{x}_cli.rs' for x in ['guard','audit','config','install','mcp']],ROOT/'rust/symguard-cli/src/doctor/config_decode.rs',ROOT/'guard/internal/config/config.go',ROOT/'migration/guard-doctor-boundary-inventory-770.md']}
OUT.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({'report':str(OUT),'cases':len(report['cases']),'warning_positive_go0':sum(c['go']['exit']==0 and bool(c.get('warning_lines')) for c in report['cases']),'warning_repeat_distinct':report['unknown_warning_repeat']['distinct_stderr'],'historical_fallback_cases':[c['argv'] for c in report['cases'] if c.get('historical_fallback_invoked')]}))
