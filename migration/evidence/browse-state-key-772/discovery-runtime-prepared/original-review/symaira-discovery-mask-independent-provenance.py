from pathlib import Path
import ast,gzip,hashlib,json,re,subprocess,tomllib
root=Path('/workspace/symaira-daemon772-state-key-discovery-mask');frozen=Path('/workspace/oracles/daemon772-go-source');sdk=Path('/workspace/toolchains/go1.26.7')
def sha(data):return hashlib.sha256(data).hexdigest()
def digest(p):return sha(p.read_bytes())
def git(*args,where=root):return subprocess.check_output(['git',*args],cwd=where,text=True).strip()
head=git('rev-parse','HEAD');assert head=='7e5c4561c82803403b579d011fb687bbe254d32a';assert not git('status','--porcelain')
author=json.load(open('/tmp/symaira-state-key-discovery-clean-718-static.json'));tracked=json.loads((root/'migration/evidence/browse-state-key-772/discovery-prepared-718/receipt.json').read_text());assert tracked==author
for p,h in author['candidate_source_sha256'].items():assert sha(subprocess.check_output(['git','show','381e324852e70fd7c647dce9bbffccb4c2731484:'+p],cwd=root))==h,p
for p,h in author['sdk_source_sha256'].items():assert digest(sdk/p)==h,p
older=json.load(open('/tmp/symaira-state-key-discovery-independent-static-provenance.json'));oldroot=Path('/workspace/symaira-daemon772-state-key-discovery');assert git('rev-parse','HEAD',where=oldroot)=='48fc3639e2265756f3bf8f9c4c914afa971ea5da';assert not git('status','--porcelain',where=oldroot)
for row in older['source_maps']:assert digest(oldroot/row['path'])==row['sha256']
for row in older['frozen_Go_files']:assert digest(frozen/row['path'])==row['sha256']
assert not git('status','--porcelain',where=frozen);assert git('rev-parse','HEAD',where=frozen)=='dcddcef0df5789123c7c9a7ebe6e01f10e941f2c'
proofs=[]
for folder,key in [('independent-ccc','raw_proofs'),('discovery-request-48fc','proofs'),('discovery-request-718','files')]:
 directory=root/'migration/evidence/browse-state-key-772'/folder;receipt=json.loads((directory/'receipt.json').read_text())
 for row in receipt[key]:
  p=directory/row['retained'];data=p.read_bytes();raw=gzip.decompress(data);assert sha(data)==row['gzip_sha256'];assert sha(raw)==row['sha256'];assert len(raw)==row['bytes']
  proofs.append(dict(folder=folder,path=str(p.relative_to(root)),gzip_sha256=sha(data),raw_sha256=sha(raw),raw_bytes=len(raw)))
assert len(proofs)==80
standalone='browse/crates/symbrowse-core/src/key_sources/command/standalone.rs';assert (root/standalone).read_bytes()==subprocess.check_output(['git','show','2b2755231b4ec5ce79de6fb2f022ac423abb44bd:'+standalone],cwd=root)
cache=Path('/home/agent/.cargo/registry/cache/index.crates.io-1949cf8c6b5b557f');lock=tomllib.loads((root/'browse/Cargo.lock').read_text());deps=[]
for name in ['same-file','winapi-util']:
 p=next(p for p in lock['package'] if p['name']==name);assert digest(cache/f"{name}-{p['version']}.crate")==p['checksum'];deps.append(p)
code=(sdk/'src/unicode/tables.go').read_text().split('var _CaseRanges = []CaseRange{',1)[1].split('\n}',1)[0];expected=[]
for lo,hi,d in re.findall(r'\{(0x[0-9A-Fa-f]+), (0x[0-9A-Fa-f]+), d\{([^}]+)\}\}',code):
 lower=d.split(',')[1].strip();delta=0x110000 if lower=='UpperLower' else int(lower)
 if delta:expected.append((int(lo,16),int(hi,16),delta))
actual=[(int(lo,16),int(hi,16),int(d)) for lo,hi,d in re.findall(r'\((0x[0-9A-Fa-f]+), (0x[0-9A-Fa-f]+), (-?\d+)\)',(root/'browse/crates/symbrowse-core/src/key_sources/startup_discovery/windows_lower.rs').read_text())];assert actual==expected and len(actual)==187
sdk_extra=['src/os/types_windows.go','src/syscall/env_windows.go','src/internal/syscall/unix/eaccess.go','src/strings/strings.go','src/unicode/letter.go']
source_paths=git('ls-files','browse/crates').splitlines();source_maps={p:digest(root/p) for p in source_paths if p.endswith(('.rs','.toml'))};source_maps.update({p:digest(root/p) for p in author['candidate_source_sha256']})
commands=[['/home/agent/.cargo/bin/rustfmt','--edition','2024','--check','browse/crates/symbrowse-core/src/key_sources/startup_discovery.rs','browse/crates/symbrowse-core/src/key_sources/command.rs'],['/workspace/toolchains/bin/actionlint','.github/workflows/browse-daemon-native.yml'],['git','diff','--check']];checks=[]
for cmd in commands:
 r=subprocess.run(cmd,cwd=root,capture_output=True,text=True);assert r.returncode==0;checks.append(dict(command=cmd,exit=r.returncode,stdout=r.stdout,stderr=r.stderr))
for p in sorted((root/'browse/port/harness').glob('discovery_*.go.in')):
 r=subprocess.run(['/workspace/toolchains/go1.26.7/bin/gofmt','-l',str(p)],capture_output=True,text=True);assert r.returncode==0 and not r.stdout;checks.append(dict(command=['gofmt','-l',str(p.relative_to(root))],exit=0,stdout='',stderr=r.stderr))
ast.parse((root/'browse/port/harness/daemon_provider_discovery.py').read_text())
archive_path=Path('/workspace/oracles/symaira-state-key-root-ccc-request/receipt.json');archive=json.loads(archive_path.read_text());shared_path=Path(archive['usage_archive_reference']);assert digest(shared_path)==archive['usage_archive_receipt_sha256'];shared=json.loads(shared_path.read_text());out=[]
for h,row in archive['unique'].items():
 p=Path(row['archive']);assert digest(p)==row['archive_sha256'];hasher=hashlib.sha256();length=0
 with gzip.open(p,'rb') as stream:
  while chunk:=stream.read(1024*1024):hasher.update(chunk);length+=len(chunk)
 assert hasher.hexdigest()==h and length==row['bytes'];out.append(dict(sha256=h,bytes=length,archive=str(p),gzip_sha256=digest(p)))
assert len(out)==187 and len(archive['records'])==247 and len(archive['actual_affected_or_build_bound_state_paths'])==61
result=dict(head=head,source='445c19688aa8f669ea99e15e6517005f6156b775',candidate_clean=True,source_maps=source_maps,author17_source_maps_verified=True,old14_source_maps_verified=True,frozen1219_files_verified=True,frozen_Go_files=older['frozen_Go_files'],original80_raw_proofs=proofs,sdk_sha256={**author['sdk_source_sha256'],**{p:digest(sdk/p) for p in sdk_extra}},Unicode15_lower187_exact=True,standalone_sha256=digest(root/standalone),dependencies=deps,checks=checks,archive_receipt_sha256=digest(archive_path),archive_247_paths_187_unique_61_actual_bindings_verified=True,archive_payloads=out,source_only=True,compiler_runtime_or_target_execution=False,targets_absent=not(root/'target').exists() and not(root/'browse/target').exists())
p=Path('/tmp/symaira-discovery-mask-independent-provenance.json');p.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(dict(status='static_provenance_pass',current_maps=len(source_maps),old_maps=14,frozen=1219,original_raw=80,sdk=len(result['sdk_sha256']),archives=len(out),runtime=0)))
