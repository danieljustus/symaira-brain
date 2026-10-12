"""Lossless review/source retention only; no product/SDK/SQLite/target execution."""
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[5]
PARENT = Path('/workspace/symaira-skills764-corrections')
REVIEW = Path('/workspace/review-proof/review-pr800-skills764-corrections')
OUT = Path(__file__).resolve().parent
HEAD = '128e1bfc81733e488e25f5b4935300a33009e265'


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def bound_rows(value):
    if isinstance(value, dict):
        if 'sha256' in value and 'bytes' in value:
            yield value
        for child in value.values():
            yield from bound_rows(child)
    elif isinstance(value, list):
        for child in value:
            yield from bound_rows(child)


def main():
    assert subprocess.check_output(['git', '-C', str(PARENT), 'rev-parse', 'HEAD']).decode().strip() == HEAD
    assert not subprocess.check_output(['git', '-C', str(PARENT), 'status', '--porcelain'])
    assert sha((REVIEW/'independent-review.md').read_bytes()) == 'd24d143bc396738f220df3e4febda79b8617246169d6530896e3ed079a3cfac9'
    assert sha((REVIEW/'independent-review-receipt.json').read_bytes()) == '739c033269e2e5fd31d1a17e33a1fe147d42e678a73c168f35c7635f98f09ec4'
    sources = {path for path in REVIEW.rglob('*') if path.is_file()}
    verified = 0
    for name in ['independent-review-receipt.json', 'verification.json', 'symaira-skills764-independent-source-review-receipt.json']:
        for row in bound_rows(json.loads((REVIEW/name).read_bytes())):
            for key in ('path', 'retained', 'original'):
                if key not in row: continue
                path = Path(row[key])
                if not path.is_absolute(): path = PARENT/path
                data = path.read_bytes()
                assert len(data) == row['bytes'] and sha(data) == row['sha256'], str(path)
                verified += 1
                sources.add(path)
    # Inherit every prior319 archived input and eight SDK gzip payloads exactly.
    old = PARENT/'migration/evidence/skills-native-764/corrections'
    sources.update(path for path in old.rglob('*') if path.is_file())
    retention = json.loads((old/'original/retention.json').read_bytes())
    with tarfile.open(old/'original/original-review.tar.gz') as packed:
        assert len(packed.getmembers()) == len(retention['members']) == 319
        for row in retention['members']:
            raw = packed.extractfile(row['archive_member']).read()
            assert sha(raw) == row['sha256'] and len(raw) == row['bytes']
    sdk = json.loads((old/'sdk-io/retention.json').read_bytes())
    for row in sdk['files']:
        packed = (PARENT/row['archive']).read_bytes(); raw = gzip.decompress(packed)
        assert sha(packed) == row['archive_sha256'] and sha(raw) == row['sha256'] and len(raw) == row['bytes']
    archive = OUT/'full-review-and-sources.tar.gz'
    members=[]
    with tarfile.open(archive,'w:gz') as saved:
        for path in sorted(sources):
            data = path.read_bytes(); metadata=path.stat()
            member='absolute/'+str(path).lstrip('/')
            saved.add(path, arcname=member, recursive=False)
            members.append({'member':member,'original':str(path),'bytes':len(data),'sha256':sha(data),
                            'mode':metadata.st_mode,'uid':metadata.st_uid,'gid':metadata.st_gid,'mtime_ns':metadata.st_mtime_ns})
        go = subprocess.check_output(['git','-C',str(PARENT),'ls-files','-z']).decode().split('\0')
        go = [name for name in go if name.endswith('.go')]
        assert len(go)==1217
        for name in go:
            raw=subprocess.check_output(['git','-C',str(PARENT),'show',HEAD+':'+name])
            assert (PARENT/name).read_bytes()==raw
            member='git/'+HEAD+'/'+name; info=tarfile.TarInfo(member); info.size=len(raw)
            saved.addfile(info,io.BytesIO(raw)); members.append({'member':member,'git_head':HEAD,'path':name,'bytes':len(raw),'sha256':sha(raw)})
    with tarfile.open(archive,'r:gz') as saved:
        assert len(saved.getmembers())==len(members)
        for row in members:
            data=saved.extractfile(row['member']).read()
            assert len(data)==row['bytes'] and sha(data)==row['sha256']
    report={'kind':'PRE_EDIT_SOURCE_RETENTION_NOT_EXECUTED_NATIVE_PROOF','parent':HEAD,
            'archive':archive.name,'archive_bytes':archive.stat().st_size,'archive_sha256':sha(archive.read_bytes()),
            'verified_external_bindings':verified,'original319_roundtrips':319,'original_SDK_gzip_roundtrips':len(sdk['files']),
            'frozen_Go_files':len(go),'members':members,'candidate_runtime_executions':0}
    (OUT/'retention.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({key:value for key,value in report.items() if key!='members'}))


if __name__=='__main__': main()
