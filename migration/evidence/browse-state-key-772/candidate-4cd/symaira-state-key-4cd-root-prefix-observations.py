import sys,json,subprocess,tempfile
from pathlib import Path
ROOT=Path('/workspace/symaira-daemon772-state-key');sys.path.insert(0,str(ROOT/'browse/port/harness'));import daemon_registry as registry
binaries={'go':Path('/tmp/symaira-pr801-go-1.26.7'),'b363':Path('/tmp/symaira-registry-b363-rust'),'candidate':Path('/workspace/symaira-daemon772-registry/target/debug/symbrowse')}
original=json.loads((ROOT/'migration/evidence/browse-state-key-772/original-b363/symaira-key-bridge-b363-baseline.json').read_text());assert registry.process.digest(binaries['b363'])==original['binaries_sha256']['rust'];rows=[]
with tempfile.TemporaryDirectory(prefix='bk-prefix-') as t:
 root=Path(t);env=registry.environment(root);env['SYMBROWSE_NO_AUTOSTART']='1'
 for group,verb in [('session','list'),('session','info'),('daemon','status'),('state','list')]:
  for output in [[],['--json'],['--output','yaml']]:
   args=['--session','owned-prefix',group,verb,*output];row={'arguments':args}
   for name,binary in binaries.items():row[name]=registry.cli(binary,root,env,args)
   row['b363_matches_candidate']=row['b363']==row['candidate'];row['go_matches_candidate']=row['go']==row['candidate'];rows.append(row)
Path('/tmp/symaira-state-key-4cd-root-prefix-observations.json').write_text(json.dumps({'candidate_source':'4cd56e3e47338aef30e5d83ea554227ace70fe71','original_b363':'b363762cb85b879630fecd3fd141cba1ae85f71c','binary_sha256':{name:registry.process.digest(binary) for name,binary in binaries.items()},'cases':rows,'scope':'private roots/no autostart; pre-existing CLI root session spelling outside newly enabled bridge; no candidate writes'},indent=2)+'\n')
print(len(rows),'literal CLI inputs',sum(not r['go_matches_candidate'] for r in rows),'Go mismatches;',sum(r['b363_matches_candidate'] for r in rows),'same as original b363')
