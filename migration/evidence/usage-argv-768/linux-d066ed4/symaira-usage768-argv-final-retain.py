from pathlib import Path
import hashlib,io,json,os,re,shutil,subprocess,tarfile
repo=Path('/workspace/symaira-usage768-argv');target=repo/'target';source='8803847278546f3161eaaa84d2e65bc2eec594a2';root=Path('/workspace/oracles/symaira-usage768-argv-880-binaries');sha=lambda data:hashlib.sha256(data).hexdigest()
assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip()==source and not subprocess.check_output(['git','status','--porcelain'],cwd=repo)
users=[]
for process in Path('/proc').iterdir():
 if not process.name.isdigit():continue
 for link in [process/'exe',process/'cwd',*list((process/'fd').glob('*'))]:
  try:value=os.readlink(link)
  except OSError:continue
  if value==str(target)or value.startswith(str(target)+'/'):users.append(dict(pid=process.name,link=str(link),value=value))
assert not users,users
local=json.load(open('/tmp/symaira-usage768-argv-final2-local.json'));parent_sha=local['argv_diagnostics']['actual']['binary_sha256']['parent'];clean=json.load(open('/tmp/symaira-usage768-argv-final2-local.evidence/argv-candidate-clean.json'))
with tarfile.open(clean['archive'],'r:gz')as bundle:
 data=bundle.extractfile('sha256/'+parent_sha).read();assert sha(data)==parent_sha;(root/'parent-usage').write_bytes(data);(root/'parent-usage').chmod(0o755)
shutil.copy2(target/'debug/symbrain',root/'candidate-usage');assert sha((root/'candidate-usage').read_bytes())==local['cli']['binary_sha256']['rust']
log=Path('/tmp/symaira-usage768-argv-final2-tests.log').read_text();paths={repo/path for path in re.findall(r'Running [^\n]* \((target/[^\n()]+)\)',log)}
assert len(paths)==33,len(paths)
paths.update([target/'debug/symbrain',target/'debug/deps/libsymbrain_usage-bc5b4536881a808f.rlib',Path('/tmp/symaira-usage768-argv-final2-public-probe'),root/'go-usage',root/'parent-usage',root/'candidate-usage'])
archive=root/'actual-executed-binaries.tar.gz';assert not archive.exists();records=[];unique=set()
with tarfile.open(archive,'w:gz',compresslevel=1)as bundle:
 for path in sorted(paths):
  data=path.read_bytes();digest=sha(data)
  if digest not in unique:
   info=tarfile.TarInfo('sha256/'+digest);info.size=len(data);info.mode=path.stat().st_mode&0o777;bundle.addfile(info,io.BytesIO(data));unique.add(digest)
  records.append(dict(path=str(path),sha256=digest,bytes=len(data)))
with tarfile.open(archive,'r:gz')as bundle:
 for row in records:
  data=bundle.extractfile('sha256/'+row['sha256']).read();assert len(data)==row['bytes']and sha(data)==row['sha256']
(root/'receipt.json').write_text(json.dumps(dict(source=source,source_clean=True,target=str(target),target_users=users,records=records,ordinary_test_binaries=33,unique_hashes=len(unique),archive=str(archive),archive_sha256=sha(archive.read_bytes()),archive_bytes=archive.stat().st_size,roundtrip_verified=True,cli_source_bound_sha256=local['cli']['binary_sha256'],parent_source='abf20713bacdab562644256e27616a9dd7acb81e',parent_from_verified_package_archive=clean['archive'],scope='Final actually executed33 ordinary test binaries plus candidate CLI/currentparent/fresh frozenGo/publicprobe/productionrlib preserved losslessly and hash/length checked. One exclusive target; all original archives unchanged.'),indent=2)+'\n')
print('PASS',len(records),'executed binarypaths/',len(unique),'unique;',archive.stat().st_size,'gzipbytes; targetusers0')
