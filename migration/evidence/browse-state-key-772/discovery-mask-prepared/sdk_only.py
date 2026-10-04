"""Explicitly authorized SDK-only Go observations; no native candidate/runtime."""
import ast
import gzip
import hashlib
import importlib.util
import itertools
import json
import os
from pathlib import Path
import shutil
import subprocess

folder = Path(__file__).resolve().parent
root = folder.parents[3]
sdk = Path('/workspace/toolchains/go1.26.7')
out = Path('/tmp/symaira-discovery-mask-sdk-only')
out.mkdir(mode=0o700, exist_ok=False)
os.chmod(out, 0o700)
spec = importlib.util.spec_from_file_location('owned_discovery_mask_projection', folder / 'projection.py')
model = importlib.util.module_from_spec(spec)
spec.loader.exec_module(model)
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
focused = ['', 'qy', 'qn', 'q0+1', 'qxy', 'qxy0', 'qxyf', 'qxya', 'qxyF', 'q!xyf', 'q-xyf',
           'qxyf+0', 'q!xyf+0', 'q0-xyf', 'qyy', 'qxy00', 'qxy9', 'q!!xyf', 'qy-xyf',
           'q!y-xyf', 'qxyf+y', 'qy+xyf', 'qxyf+0+1', 'q0+1-xyf', 'qxyf+y-y', 'q-xyf-y', 'q-xyf-0']
original = set('q' + ''.join(chars) for n in range(1, 5) for chars in itertools.product('01yxf+-', repeat=n))
original.update('q!' + p[1:] for p in list(original))
patterns = sorted(original | set(focused))
(out / 'input.json').write_text(json.dumps(patterns) + '\n')
projection = []
for pattern in patterns:
    sdk_result, native_result = model.sdk_result(pattern), model.native_result(pattern)
    if sdk_result['allow_relative'] is not None:
        assert native_result['allow_relative'] == sdk_result['allow_relative'], (pattern, sdk_result, native_result)
    projection.append(dict(pattern=pattern, sdk=sdk_result, native_projection=native_result))
(out / 'projection.json').write_text(json.dumps(projection, indent=2) + '\n')
home = out / 'home'; home.mkdir(mode=0o700)
cache = out / 'cache'; cache.mkdir(mode=0o700)
environment = {**os.environ, 'HOME': str(home), 'GOCACHE': str(cache), 'GOENV': 'off', 'GOPROXY': 'off',
               'GOSUMDB': 'off', 'GOTOOLCHAIN': 'local', 'GO111MODULE': 'off', 'CGO_ENABLED': '0', 'GOMAXPROCS': '2'}
bindings = {str(path): sha(path) for path in [sdk / 'src/internal/bisect/bisect.go', sdk / 'src/internal/godebug/godebug.go', sdk / 'LICENSE', folder / 'sdk_bulk.go.in', folder / 'projection.py', root / 'browse/crates/symbrowse-core/src/key_sources/startup_discovery/godebug.rs']}
results = []
for mode in ['sdk', 'actual-mutant']:
    assert shutil.disk_usage(out).free >= 700 * 1024**2, '700MiB floor before SDK-only stage'
    source = out / mode; source.mkdir(mode=0o700)
    package = source / 'bisect'; package.mkdir()
    shutil.copyfile(folder / 'sdk_bulk.go.in', source / 'main.go')
    data = (sdk / 'src/internal/bisect/bisect.go').read_bytes()
    if mode == 'actual-mutant':
        # Corrupt only the owned SDK copy, never the SDK or frozen Go files.
        before, after = b'if id&c.mask == c.bits {', b'if id&c.mask == c.bits&c.mask {'
        assert data.count(before) == 1
        data = data.replace(before, after)
    (package / 'bisect.go').write_bytes(data)
    shutil.copyfile(sdk / 'LICENSE', source / 'LICENSE')
    binary = out / (mode + '-typed-bisect')
    command = [str(sdk / 'bin/go'), 'build', '-p', '2', '-o', str(binary), str(source / 'main.go')]
    build = subprocess.run(command, cwd=source, env=environment, capture_output=True, timeout=120)
    (out / (mode + '-build.stdout')).write_bytes(build.stdout)
    (out / (mode + '-build.stderr')).write_bytes(build.stderr)
    assert build.returncode == 0, build.stderr
    run = subprocess.run([str(binary)], input=(out / 'input.json').read_bytes(), cwd=source,
                         env=environment, capture_output=True, timeout=30)
    (out / (mode + '-stdout.json')).write_bytes(run.stdout)
    (out / (mode + '-stderr')).write_bytes(run.stderr)
    assert run.returncode == 0 and not run.stderr
    rows = json.loads(run.stdout)
    assert len(rows) == len(patterns)
    differences = []
    for row in rows:
        parsed = model.sdk_new(row['pattern'])
        assert row['nil_matcher'] == (parsed is None)
        for value in row['typed_ids']:
            if parsed is None:
                enabled, printing = True, False
            else:
                quiet, enable, conditions = parsed
                matched = next((result for mask, bits, result in reversed(conditions) if value['id'] & mask == bits), False)
                enabled, printing = matched == enable, not quiet and matched
            if (value['enabled'], value['print']) != (enabled, printing):
                differences.append(dict(pattern=row['pattern'], id=value['id'], expected=[enabled, printing], actual=[value['enabled'], value['print']]))
    if mode == 'sdk':
        assert not differences, differences
    else:
        assert any(row['pattern'] == 'qxyf' for row in differences)
    result = dict(mode=mode, build_command=command, build_exit=build.returncode, run_exit=run.returncode,
                  binary_sha256=sha(binary), actual_copied_bisect_sha256=sha(package / 'bisect.go'),
                  typed_observations=sum(len(r['typed_ids']) for r in rows), differences=differences)
    (out / (mode + '-result.json')).write_text(json.dumps(result, indent=2) + '\n')
    results.append(result)
    # Cache is strictly owned ephemeral SDK graph; keep actual binaries/sources/proofs.
shutil.rmtree(cache)
receipt = dict(scope='Actual Go SDK-only typed matcher observations plus separate native source projection; no Rust candidate/provider/native acceptance',
               sdk_version=subprocess.check_output([str(sdk / 'bin/go'), 'version'], env=environment, text=True).strip(),
               sdk_source_bindings=bindings, focused_patterns=focused, original_patterns=len(original), total_patterns=len(patterns),
               native_source_projection_mismatches=0, candidate_rust_builds=0, provider_processes=0, targets=0, ports=0,
               actual_sdk_runs=results, actual_mutant_is_owned_sdk_predicate_copy=True,
               limitations='SDK typed IDs prove concrete outputs, not Rust runtime equivalence. Original finite grammar projection covers all bounded suffix classes; conditional/reporting stack and native Windows ownership remain open.')
(out / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps({'total_patterns':len(patterns), 'original_patterns':len(original), 'actual_typed_observations_per_binary':results[0]['typed_observations'], 'actual_mutant_differences':len(results[1]['differences']), 'source_projection_mismatches':0}))
