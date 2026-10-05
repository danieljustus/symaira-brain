#!/usr/bin/env python3
"""Prepared actual gate driver; allocation is required before any execution."""
from pathlib import Path
import argparse, gzip, hashlib, json, os, shutil, subprocess, time

ROOT = Path(__file__).resolve().parents[4]
BROWSE = ROOT / 'browse'
HERE = Path(__file__).resolve().parent
TARGET = ROOT / 'target'
SDK_GO = '/workspace/toolchains/go1.26.7/bin/go'
FROZEN = Path('/workspace/oracles/daemon772-go-source')
UPSTREAM = '7e5c4561c82803403b579d011fb687bbe254d32a'


def digest(path):
    h = hashlib.sha256()
    with path.open('rb') as stream:
        while chunk := stream.read(1024 * 1024): h.update(chunk)
    return h.hexdigest()


def git(*args):
    return subprocess.check_output(['git', '-C', str(ROOT), *args], text=True).strip()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--phase', choices=['nonport', 'port'], required=True)
    parser.add_argument('--allocation-receipt', type=Path, required=True)
    parser.add_argument('--port-release-receipt', type=Path)
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    allocation = json.loads(args.allocation_receipt.read_text())
    assert allocation['target'] == str(TARGET) and allocation['exclusive_owner'] == 'memory758_port'
    assert allocation['authorized'] is True and TARGET.is_dir()
    assert os.environ.get('CARGO_TARGET_DIR') == str(TARGET)
    assert os.environ.get('CARGO_BUILD_JOBS') == '2'
    assert os.environ.get('CARGO_INCREMENTAL') == '0'
    assert os.environ.get('CARGO_PROFILE_DEV_DEBUG') == '0'
    assert os.environ.get('CARGO_PROFILE_TEST_DEBUG') == '0'
    if args.phase == 'port':
        assert args.port_release_receipt is not None
        port = json.loads(args.port_release_receipt.read_text())
        assert port['port'] == 11434 and port['exclusive_owner'] == 'memory758_port' and port['authorized'] is True
        assert not subprocess.check_output(['ss', '-ltnH', 'sport = :11434'], text=True).strip()
    source = git('rev-parse', 'HEAD'); assert not git('status', '--porcelain')
    subprocess.run(['git', '-C', str(ROOT), 'merge-base', '--is-ancestor', UPSTREAM, source], check=True)
    args.out.mkdir(parents=True, exist_ok=False)
    state = {'phase': args.phase, 'source': source, 'upstream': UPSTREAM,
             'allocation_receipt_sha256': digest(args.allocation_receipt), 'stages': [], 'complete': False}
    state['source_sha256'] = {p: digest(ROOT / p) for p in git('ls-files', 'browse/crates', 'browse/port/harness', 'browse/Cargo.lock', 'browse/Cargo.toml', '.github/workflows/browse-daemon-native.yml').splitlines()}
    def flush():
        (args.out / 'validation.json').write_text(json.dumps(state, indent=2) + '\n')
    def preserve_failure():
        archives = args.out / 'failed-executables'; archives.mkdir(exist_ok=True)
        rows = []; unique = {}
        for path in TARGET.rglob('*'):
            if not path.is_file(): continue
            with path.open('rb') as f: magic = f.read(4)
            if magic != b'\x7fELF': continue
            h = digest(path)
            if h not in unique:
                archive = archives / (h + '.gz')
                with path.open('rb') as src, archive.open('wb') as raw:
                    with gzip.GzipFile(fileobj=raw, mode='wb', mtime=0) as dst: shutil.copyfileobj(src, dst)
                restored = hashlib.sha256()
                with gzip.open(archive, 'rb') as f:
                    while c := f.read(1024 * 1024): restored.update(c)
                assert restored.hexdigest() == h
                unique[h] = {'archive': str(archive), 'gzip_sha256': digest(archive)}
            rows.append({'path': str(path), 'sha256': h, 'bytes': path.stat().st_size})
        state['failed_source_bound_ELF'] = {'source': source, 'paths': rows, 'unique': unique}
        flush()
    def run(name, command, cwd=BROWSE):
        available = shutil.disk_usage('/workspace').free
        assert available >= 700 * 1024 * 1024, ('global stage floor', name, available)
        assert git('rev-parse', 'HEAD') == source and not git('status', '--porcelain')
        row = {'name': name, 'command': command, 'cwd': str(cwd), 'free_bytes_before': available,
               'started_unix': time.time(), 'status': 'running'}
        state['stages'].append(row); flush()
        log = args.out / (name + '.log')
        print('START', name, flush=True)
        with log.open('wb') as output: result = subprocess.run(command, cwd=cwd, stdout=output, stderr=subprocess.STDOUT)
        row.update(exit=result.returncode, completed_unix=time.time(), log_sha256=digest(log), status='passed' if result.returncode == 0 else 'failed')
        flush()
        if result.returncode:
            preserve_failure(); raise SystemExit(result.returncode)
        print('PASS', name, flush=True)
    harness = BROWSE / 'port/harness'
    owned = ['python3', str(harness / 'key_test_environment.py'), '--go', SDK_GO, '--']
    if args.phase == 'nonport':
        run('fmt', ['cargo', 'fmt', '--all', '--', '--check'])
        run('strict', ['cargo', 'clippy', '--workspace', '--all-targets', '--all-features', '--locked', '--', '-D', 'warnings'])
        run('affected-original', owned + ['cargo', 'test', '-p', 'symbrowse-core', '-p', 'symbrowse-protocol', '-p', 'symbrowse-daemon', '-p', 'symbrowse-cli', '--all-features', '--locked'])
        run('affected-all-targets', owned + ['cargo', 'test', '-p', 'symbrowse-core', '-p', 'symbrowse-protocol', '-p', 'symbrowse-daemon', '-p', 'symbrowse-cli', '--all-targets', '--all-features', '--locked'])
        run('mcp-lib', owned + ['cargo', 'test', '-p', 'symbrowse-mcp', '--lib', '--all-features', '--locked'])
        run('build-cli', ['cargo', 'build', '-p', 'symbrowse-cli', '--bin', 'symbrowse', '--locked'])
        run('build-probes', ['cargo', 'build', '-p', 'symbrowse-core', '--example', 'state_key_store_probe', '--example', 'startup_key_source_probe', '--locked'])
        run('windows-static', ['cargo', 'clippy', '-p', 'symbrowse-core', '--all-targets', '--all-features', '--target', 'x86_64-pc-windows-gnu', '--locked', '--', '-D', 'warnings'])
        run('darwin-static', ['cargo', 'check', '-p', 'symbrowse-core', '--all-targets', '--all-features', '--target', 'x86_64-apple-darwin', '--locked'])
        run('actionlint', ['/workspace/toolchains/bin/actionlint', '.github/workflows/browse-daemon-native.yml'], ROOT)
        run('matrix', ['python3', 'browse/docs/rust-port/validate.py'], ROOT)
        for name in ['test_windows_pipe.py', 'test_registry_progress.py']:
            run(name.replace('.py', ''), ['python3', str(harness / name)])
        run('portable', ['python3', str(harness / 'test_run.py'), 'DaemonExitTests', 'CargoTargetRootTests.test_ci_default_target_stays_in_browse_root', 'CargoTargetRootTests.test_ci_report_path_remains_portable', 'CargoTargetRootTests.test_relative_target_is_resolved_against_browse_root', 'CargoTargetRootTests.test_absolute_target_is_preserved'])
    else:
        rust = str(TARGET / 'debug/symbrowse'); probe = str(TARGET / 'debug/examples/startup_key_source_probe')
        go = '/tmp/symaira-pr801-go-1.26.7'; fixture = '/tmp/symaira-pr801-go-mcp-v0.8.0'
        state['bound_oracle_sha256'] = {p: digest(Path(p)) for p in [go, fixture]}
        state['bound_native_sha256'] = {p: digest(Path(p)) for p in [rust, probe, str(TARGET / 'debug/examples/state_key_store_probe')]}; flush()
        retained = args.out / 'retained-provider'
        provider_source = args.out / 'retained-provider.go'
        shutil.copyfile(HERE / 'original-driver/retained-provider.go.in', provider_source)
        state['retained_provider_source_sha256'] = digest(provider_source); flush()
        run('build-original-provider', [SDK_GO, 'build', '-o', str(retained), str(provider_source)], ROOT)
        run('ownership', ['python3', str(harness / 'daemon_state_key_ownership.py'), '--rust', rust, '--rust-source-probe', probe, '--go-tool', SDK_GO, '--out', str(args.out / 'ownership.json')])
        shared = ['--go', go, '--rust', rust, '--go-source', str(FROZEN)]
        run('key', ['python3', str(harness / 'daemon_state_key.py'), *shared, '--rust-store-probe', str(TARGET / 'debug/examples/state_key_store_probe'), '--go-tool', SDK_GO, '--out', str(args.out / 'key.json')])
        for name in ['process', 'registry']:
            run(name, ['python3', str(harness / ('daemon_' + name + '.py')), *shared, '--out', str(args.out / (name + '.json'))])
        run('discovery', ['python3', str(harness / 'daemon_provider_discovery.py'), *shared, '--rust-source-probe', probe, '--go-tool', SDK_GO, '--retained-provider', str(retained), '--out', str(args.out / 'discovery.json')])
        run('mcp', ['python3', str(harness / 'daemon_mcp.py'), *shared, '--go-fixture', fixture, '--workspace', '--out', str(args.out / 'mcp.json')])
    assert git('rev-parse', 'HEAD') == source and not git('status', '--porcelain')
    state['complete'] = True; flush()

if __name__ == '__main__': main()
