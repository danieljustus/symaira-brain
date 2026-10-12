"""Retain complete inspected SDK/dependency owners as bytes, no SDK execution."""
import hashlib
import json
from pathlib import Path
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent
GO = Path('/workspace/toolchains/go1.26.7/src')
CACHE = Path('/home/agent/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f')


def sha(raw): return hashlib.sha256(raw).hexdigest()


def main():
    sdk = [GO/name for name in ['strings/strings.go','unicode/graphic.go','unicode/tables.go',
                                'syscall/syscall_windows.go','syscall/wtf8_windows.go','encoding/json/encode.go','encoding/json/decode.go']]
    modules = {
        'cap-std': ['src/fs/dir.rs'],
        'cap-primitives': ['src/fs/stat.rs','src/fs/mod.rs','src/fs/manually/open.rs','src/fs/manually/mod.rs',
                           'src/rustix/linux/fs/stat_impl.rs','src/rustix/linux/fs/mod.rs','src/rustix/fs/stat_unchecked.rs','src/rustix/fs/mod.rs'],
        'serde_json': ['src/de.rs','src/read.rs','src/ser.rs','src/raw.rs','src/value/ser.rs'],
    }
    lock = tomllib.loads((ROOT/'Cargo.lock').read_text())
    packages = {}
    files = list(sdk)
    for name, members in modules.items():
        selected = next(row for row in lock['package'] if row['name'] == name)
        directory = CACHE/(name+'-'+selected['version'])
        crate = CACHE.parent.parent/'cache'/CACHE.name/(name+'-'+selected['version']+'.crate')
        assert sha(crate.read_bytes()) == selected['checksum']
        inspected = [directory/'Cargo.toml'] + [directory/member for member in members]
        with tarfile.open(crate) as published:
            for path in inspected:
                assert published.extractfile(name+'-'+selected['version']+'/'+str(path.relative_to(directory))).read() == path.read_bytes()
        packages[name] = {**selected, 'actual_crate':str(crate),'bytes':crate.stat().st_size}
        files += [crate, *inspected]
    archive = OUT/'inspected-sdk-dependency-owners.tar.gz'
    rows = []
    with tarfile.open(archive,'w:gz') as packed:
        for path in files:
            raw = path.read_bytes(); stat = path.stat(); member = str(path).lstrip('/')
            packed.add(path,arcname=member,recursive=False)
            rows.append({'path':str(path),'member':member,'bytes':len(raw),'sha256':sha(raw),
                         'mode':stat.st_mode,'uid':stat.st_uid,'gid':stat.st_gid,'mtime_ns':stat.st_mtime_ns})
    with tarfile.open(archive) as packed:
        for row in rows:
            raw = packed.extractfile(row['member']).read()
            assert len(raw)==row['bytes'] and sha(raw)==row['sha256']
    report = {'kind':'SOURCE_REFERENCES_NOT_SDK_OR_NATIVE_EXECUTION','archive':archive.name,
              'bytes':archive.stat().st_size,'sha256':sha(archive.read_bytes()),'files':rows,'locked_packages':packages,
              'full_roundtrips':len(rows),'product_SDK_compiler_executions':0}
    (OUT/'boundary-source-retention.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({key:value for key,value in report.items() if key not in ('files','locked_packages')}))


if __name__=='__main__': main()
