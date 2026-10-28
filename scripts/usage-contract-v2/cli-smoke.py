#!/usr/bin/env python3
"""Native macOS synthetic CLI observations; network denied, not provider/account acceptance."""
from pathlib import Path
from datetime import datetime,timezone
import os,subprocess,json,time,hashlib,stat,copy,sys
E=Path(sys.argv[1]).resolve();E.mkdir()
B=Path(sys.argv[2]).resolve(); C=Path(sys.argv[3]).resolve()
S=B/'grok-history-smoke'/E.name;S.mkdir(parents=True)
traps=S/'traps';traps.mkdir();marker=S/'tool-accessed'
for tool in ['symvault','security','ps','lsof','gh','go-fallback']:
    p=traps/tool;p.write_text(f"#!/bin/sh\nprintf accessed >> '{marker}'\nexit 1\n");p.chmod(0o700)
def inventory(root):
    out={}
    for p in sorted(root.rglob('*')):
        s=p.lstat();row=dict(mode=stat.S_IMODE(s.st_mode),type=stat.S_IFMT(s.st_mode),size=s.st_size,mtime_ns=s.st_mtime_ns)
        if stat.S_ISREG(s.st_mode): row['sha256']=hashlib.sha256(p.read_bytes()).hexdigest()
        if stat.S_ISLNK(s.st_mode): row['link']=str(p.readlink())
        out[str(p.relative_to(root))]=row
    return out
now=datetime.now(timezone.utc).isoformat()
event={'timestamp':now,'params':{'_meta':{'eventId':'synthetic-private-event-id'},'update':{'sessionUpdate':'turn_completed','usage':{'modelUsage':{'grok-build':{'inputTokens':100,'outputTokens':20,'reasoningTokens':5,'costUsdTicks':2357158800}}}},'prompt':'synthetic-never-output'}}
results=[];outputs={}
cases=['missing-auth','expired-auth','api-network-denied','disabled','unselected','invalid-selector','no-data','true-zero','missing-cost','conflict','tail','symlink','fifo','table']
for lang,binary in [('go',B/'symbrain-go'),('rust',C/'debug/symbrain')]:
    for case in cases:
        root=S/lang/case;root.mkdir(parents=True);home=root/'home';home.mkdir();runtime=root/'runtime';runtime.mkdir();grok=home/'.grok';ledger=grok/'sessions/p/updates.jsonl';ledger.parent.mkdir(parents=True)
        env={'HOME':str(home),'USERPROFILE':str(home),'PATH':str(traps),'TMPDIR':str(runtime),'TZ':'Europe/Berlin','GROK_HOME':str(grok),'SYMBRAIN_USAGE_GROK_HISTORY':'1','SYMBRAIN_GO_BINARY':str(traps/'go-fallback'),'XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache')}
        e=copy.deepcopy(event);selector='grok';fmt='--json'
        if case=='true-zero': e['params']['update']['usage']['modelUsage']['grok-build']={'inputTokens':0,'outputTokens':0,'costUsdTicks':0}
        if case=='missing-cost': del e['params']['update']['usage']['modelUsage']['grok-build']['costUsdTicks']
        ledger.write_text(json.dumps(e)+'\n')
        if case=='expired-auth': env['GROK_ACCESS_TOKEN']='x.eyJleHAiOjF9.x'
        if case=='api-network-denied': env['GROK_ACCESS_TOKEN']='synthetic-offline-token'
        if case=='disabled': env.pop('SYMBRAIN_USAGE_GROK_HISTORY')
        if case=='unselected': selector='zai';env['GROK_ACCESS_TOKEN']='symvault://synthetic/must-not-resolve'
        if case=='invalid-selector': selector='grok,unknown';env['GROK_ACCESS_TOKEN']='symvault://synthetic/must-not-resolve'
        if case=='no-data': ledger.unlink()
        if case=='conflict':
            e['params']['update']['usage']['modelUsage']['grok-build']['inputTokens']=101
            with ledger.open('a') as f:f.write(json.dumps(e)+'\n')
        if case=='tail':
            with ledger.open('a') as f:f.write('{unterminated')
        if case=='symlink':
            outside=root/'outside';outside.write_text(json.dumps(e)+'\n');ledger.unlink();ledger.symlink_to(outside)
        if case=='fifo': ledger.unlink();os.mkfifo(ledger,0o600)
        args=[str(binary),'usage','--providers',selector]
        if case!='table':args.append('--json')
        command=['/usr/bin/sandbox-exec','-p','(version 1) (allow default) (deny network*)',*args]
        before=inventory(root);start=time.monotonic();run=subprocess.run(command,env=env,capture_output=True,timeout=15);seconds=time.monotonic()-start
        stem=lang+'-'+case;(E/(stem+'.stdout')).write_bytes(run.stdout);(E/(stem+'.stderr')).write_bytes(run.stderr)
        if case=='invalid-selector': assert run.returncode==2 and not run.stdout
        else:
            assert run.returncode==0,(lang,case,run.stderr)
            assert not run.stderr,(lang,case,run.stderr)
            if case=='table':
                assert b'local history\tavailable\tscope=local_machine attribution=unattributed' in run.stdout
                assert b'120 tokens\t0.23571588 USD recorded' in run.stdout
            else:
                report=json.loads(run.stdout);assert report['schema_version']==1 and len(report['providers'])==1
                p=report['providers'][0];assert p['id']==selector
                assert 'snapshot' not in p
                assert p['configured']==(case=='api-network-denied')
                assert ('error' in p)==(case=='api-network-denied')
                if case in ['disabled','unselected']: assert 'history' not in p
                else:
                    h=p['history'];assert h['scope']=='local_machine' and h['attribution']=='unattributed' and h['cost_basis']=='recorded' and h['currency']=='USD'
                    expected='no_data' if case=='no-data' else 'unavailable' if case in ['symlink','fifo'] else 'partial' if case in ['missing-cost','conflict','tail'] else 'available'
                    assert h['status']==expected,(lang,case,h)
                    if case in ['no-data','conflict','symlink','fifo']: assert not h['daily'] and not h['periods']
                    else:
                        m=h['periods'][0]['models'][0]
                        assert m['tokens']==('0' if case=='true-zero' else '120')
                        if case=='missing-cost':assert 'recorded_cost' not in m and m['missing_cost']==1
                        else:assert m['recorded_cost']==('0' if case=='true-zero' else '0.23571588')
                outputs[(lang,case)]=report
        for private in ['synthetic-private-event-id','synthetic-never-output','synthetic-offline-token',str(root)]:assert private.encode() not in run.stdout
        after=inventory(root);assert before==after,(lang,case,'input mutation');assert not marker.exists();assert not list(runtime.iterdir())
        (E/(stem+'.inventory.json')).write_text(json.dumps({'before':before,'after':after},indent=2))
        results.append(dict(language=lang,case=case,command=command,exit_code=run.returncode,seconds=seconds,success=True))
for case in cases:
    if ('go',case) in outputs:
        assert outputs[('go',case)]==outputs[('rust',case)],case
assert len(results)==len(cases)*2
(E/'cli-smoke.json').write_text(json.dumps(results,indent=2))
identities={lang:dict(path=str(p),sha256=hashlib.sha256(p.read_bytes()).hexdigest()) for lang,p in [('go',B/'symbrain-go'),('rust',C/'debug/symbrain')]}
(E/'binary-sha256.json').write_text(json.dumps(identities,indent=2));print(json.dumps(dict(passed=len(results),evidence=str(E),binaries=identities),indent=2))
