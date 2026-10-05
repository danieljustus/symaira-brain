from pathlib import Path
import hashlib,json,subprocess,tarfile,re
r=Path('/workspace/symaira-guard770-raw-paths');h=lambda b:hashlib.sha256(b).hexdigest();f=lambda p:h(Path(p).read_bytes());git=lambda *a:subprocess.check_output(['git',*a],cwd=r)
head='208aec97423392749f6d947a91977d2c2b092dd2';source='806ddcf8bd36c57b5b26d5148cf7015a465d289e';assert git('rev-parse','HEAD').decode().strip()==head and not git('status','--porcelain')
v=json.loads((r/'migration/evidence/guard-raw-paths-770/final-806/symaira-guard770-raw-final-verification.json').read_text());assert len(v['candidate_source_sha256'])==96
for p,x in v['candidate_source_sha256'].items():assert f(r/p)==x and h(git('show',source+':'+p))==x
for p,x in v['proof_sha256'].items():assert f(p)==x;tracked=r/'migration/evidence/guard-raw-paths-770/final-806'/Path(p).name;assert f(tracked)==x
for p,x in v['binaries'].items():assert f(p)==x['sha256']
old=r/'migration/evidence/guard-raw-paths-770/original-e876';pres=json.loads((old/'preservation.json').read_text());assert len(pres['mapping'])==34
for row in pres['mapping']:assert f(r/row['retained'])==row['sha256']
a=json.loads((old/'receipt.json').read_text());assert f(a['archive'])==a['archive_sha256'] and len(a['native_ELFs'])==53
with tarfile.open(a['archive']) as t:
 for p,x in a['native_ELFs'].items():data=t.extractfile(p).read();assert h(data)==x['sha256'] and len(data)==x['bytes']
p=json.loads(Path('/tmp/symaira-guard806-root-process.json').read_text());assert p['candidate_head']==head and not p['candidate_dirty'] and p['total']==124 and p['matched']==121 and len(p['remaining_native_diagnostic_states'])==3 and not p['audit_diagnostic_deviations']
assert len(p['go_source_sha256'])==1217
for name,x in p['go_source_sha256'].items():assert h(git('show',p['oracle_ref']+':'+name))==x and f(Path('/tmp/symaira-guard770-diagnostics-independent-go-source')/name)==x
for name,x in p['candidate_source_sha256'].items():assert f(r/name)==x
for n,matched in [(80,80),(94,91)]:
 d=json.loads(Path(f'/tmp/symaira-guard806-root-original{n}.json').read_text());assert d['candidate_head']==head and not d['candidate_dirty'] and d['total']==n and d['matched']==matched
x=json.loads(Path('/tmp/symaira-guard806-root-extra.json').read_text());assert (x['total'],x['matched'],x['gated'],x['failed'])==(31,27,4,0)
for suffix in ['raw-paths','raw-paths-unicode']:
 d=json.loads(Path('/tmp/symaira-guard806-root-'+suffix+'.json').read_text());assert len(d['results'])==4 and all(x['matched'] for x in d['results'])
x=json.loads(Path('/tmp/symaira-guard806-root-process-raw-paths.json').read_text());c=json.loads(Path('/tmp/symaira-guard806-root-process-controls.json').read_text());assert x['total']==x['matched']==63 and len(x['controls'])==2 and c['rejected']==3
assert all(y['rejected'] and y['mutated_native']['stdout_hex'] and not y['mutated_native']['stderr_hex'] for y in x['controls']+c['controls'])
x=json.loads(Path('/tmp/symaira-guard806-root-extra-mixed.json').read_text());assert x['total']==x['matched']==35
x=json.loads(Path('/tmp/symaira-guard806-root-audit-filesystem.json').read_text());assert x['cases']==6 and all(y['native_invariant_passed'] for y in x['results'])
counts=[]
for name in ['tests','kernel-tests']:counts+=re.findall(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;',Path('/tmp/symaira-guard806-root-'+name+'.log').read_text())
assert sum(int(x[0]) for x in counts)==168 and len(counts)==13 and not any(int(x[1])+int(x[2]) for x in counts)
assert not git('diff',source,head,'--','rust','scripts','.github','docs')
assert not git('diff','e8766e3',head,'--','Cargo.toml','Cargo.lock','rust/symbrain-audit','rust/symbrain-guard-core')
print('PASS immutable208/source806:96candidate1217Go/25freshauthor34originalproofs/14current53archivedELFs;168tests13summaries;124121+3gates;80/94originals;63+2mutants3controls;31ordered4raw4Unicode6audit;35newmixedbytepairs;alloriginals/frozen/pins/kernelunchanged')
