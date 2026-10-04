"""Preserve package-owned artifacts before an exclusive-target CLI rebuild."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile


def target_users(target):
    users = []
    if Path('/proc').is_dir():
        for process in Path('/proc').iterdir():
            if not process.name.isdigit():
                continue
            for link in [process / 'exe', process / 'cwd', *list((process / 'fd').glob('*'))]:
                try:
                    value = os.readlink(link)
                except OSError:
                    continue
                if value == str(target) or value.startswith(str(target) + '/'):
                    users.append(dict(pid=process.name, link=str(link), value=value))
        assert not users, users
    return users


def main():
    target = Path(sys.argv[1]).resolve()
    root = Path(sys.argv[2]).resolve()
    output = Path(sys.argv[3]).resolve()
    assert target.name == 'target' and target != root and target.is_dir()
    users = target_users(target)
    command = ['cargo', 'clean', '--locked', '-p', 'symbrain-cli', '--target-dir', str(target)]
    dry = subprocess.run(command + ['--dry-run', '--verbose'], cwd=root, capture_output=True, check=True)
    output.with_suffix('.dry-run.log').write_bytes(dry.stdout + dry.stderr)
    files = []
    for line in (dry.stdout + dry.stderr).decode(errors='strict').splitlines():
        path = Path(line)
        if not path.is_absolute():
            continue
        assert path.is_relative_to(target), line
        if path.is_file():
            files.append(path)
    # Cargo package clean need not list the top-level hard-linked executable;
    # the next build still overwrites it, so preserve those actual bytes too.
    files.extend(path for path in [target / 'debug/symbrain', target / 'debug/symbrain.exe'] if path.is_file())
    records, unique = [], {}
    archive = output.with_suffix('.tar.gz')
    assert not output.exists() and not archive.exists()
    with tarfile.open(archive, 'w:gz', compresslevel=1) as bundle:
        for path in sorted(set(files)):
            data = path.read_bytes()
            digest = hashlib.sha256(data).hexdigest()
            if digest not in unique:
                info = tarfile.TarInfo('sha256/' + digest)
                info.size = len(data)
                info.mode = path.stat().st_mode & 0o777
                bundle.addfile(info, io.BytesIO(data))
                unique[digest] = len(data)
            records.append(dict(path=str(path), sha256=digest, bytes=len(data)))
    with tarfile.open(archive, 'r:gz') as bundle:
        for digest, length in unique.items():
            data = bundle.extractfile('sha256/' + digest).read()
            assert len(data) == length and hashlib.sha256(data).hexdigest() == digest
    receipt = dict(target=str(target), package='symbrain-cli', target_users=users,
                   proc_check=Path('/proc').is_dir(), records=records, unique_files=len(unique),
                   archive=str(archive), archive_bytes=archive.stat().st_size,
                   archive_sha256=hashlib.sha256(archive.read_bytes()).hexdigest(), roundtrip_verified=True,
                   command=command, scope='One runner-owned exclusive target, all cargo dry-run regular files losslessly retained before package-only clean; no other package/target cleaned. /proc check is Linux-only, not a Windows lifecycle proof.')
    output.write_text(json.dumps(receipt, indent=2) + '\n')
    assert not target_users(target)
    for row in records:
        assert hashlib.sha256(Path(row['path']).read_bytes()).hexdigest() == row['sha256'], row
    clean = subprocess.run(command, cwd=root, capture_output=True, check=True)
    output.with_suffix('.clean.log').write_bytes(clean.stdout + clean.stderr)
    print('Preserved CLI package:', len(records), 'paths/', len(unique), 'unique files before package-only clean')


if __name__ == '__main__':
    main()
