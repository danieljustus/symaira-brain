"""Executed pure SOURCE checks only; no product/SDK/compiler/SQLite/target use."""
import ast
import gzip
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile

ROOT = Path(__file__).resolve().parents[5]
OUT = Path(__file__).resolve().parent
PARENT = '128e1bfc81733e488e25f5b4935300a33009e265'
REVIEW = Path('/workspace/review-proof/review-pr800-skills764-corrections')


def sha(raw): return hashlib.sha256(raw).hexdigest()
def git(*args): return subprocess.check_output(['git', '-C', str(ROOT), *args])
def parent(name): return git('show', PARENT + ':' + name)
def tree(text): return ast.dump(ast.parse(text), include_attributes=False)
def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def case_projection(text, env):
    entry = next(node for node in ast.parse(text).body if isinstance(node, ast.FunctionDef) and node.name == 'main')
    assignments = []
    for node in ast.walk(entry):
        if isinstance(node, (ast.Assign, ast.AugAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if len(targets) == 1 and isinstance(targets[0], ast.Name) and targets[0].id == 'cases': assignments.append(node)
    assignments.sort(key=lambda node: node.lineno)
    exec(compile(ast.Module(body=assignments, type_ignores=[]), '<case plans only>', 'exec'), env)
    return env['cases']


def main():
    os.umask(0o022)
    assert os.statvfs(ROOT).f_bavail * os.statvfs(ROOT).f_frsize >= 700 * 1024 * 1024
    report = {'kind': 'PURE_SOURCE_CHECKS_NOT_NATIVE_OR_COMPILER_VALIDATION',
              'parent': PARENT, 'product_SDK_compiler_SQLite_target_port_executions': 0}
    retention = json.loads((OUT.parent / 'original/retention.json').read_bytes())
    archive = OUT.parent / 'original/full-review-and-sources.tar.gz'
    assert sha(archive.read_bytes()) == retention['archive_sha256']
    with tarfile.open(archive) as saved:
        assert len(saved.getmembers()) == len(retention['members'])
        for row in retention['members']:
            item = saved.getmember(row['member']); raw = saved.extractfile(item).read()
            assert len(raw) == row['bytes'] and sha(raw) == row['sha256']
            if 'original' in row:
                original = Path(row['original']); stat = original.stat()
                assert original.read_bytes() == raw
                assert (stat.st_mode, stat.st_uid, stat.st_gid, stat.st_mtime_ns) == (row['mode'], row['uid'], row['gid'], row['mtime_ns'])
                assert (item.mode, item.uid, item.gid) == (row['mode'] & 0o7777, row['uid'], row['gid'])
    report['all_pre_edit_members_bytes_and_metadata_ledger_verified'] = len(retention['members'])
    assert sha((REVIEW/'independent-review.md').read_bytes()) == 'd24d143bc396738f220df3e4febda79b8617246169d6530896e3ed079a3cfac9'
    assert sha((REVIEW/'independent-review-receipt.json').read_bytes()) == '739c033269e2e5fd31d1a17e33a1fe147d42e678a73c168f35c7635f98f09ec4'
    assert git('-C', '/workspace/symaira-skills764-corrections', 'rev-parse', 'HEAD').decode().strip() == PARENT
    assert not git('-C', '/workspace/symaira-skills764-corrections', 'status', '--porcelain')
    # Git-object reads keep the sparse checkout small and never materialize Go.
    names = [name for name in git('ls-files').decode().splitlines() if name.endswith('.go')]
    old_names = [name for name in git('ls-tree', '-r', '--name-only', PARENT).decode().splitlines() if name.endswith('.go')]
    assert names == old_names and len(names) == 1217
    def blobs(revision):
        stream = subprocess.check_output(
            ['git', '-C', str(ROOT), 'cat-file', '--batch'], input=''.join(revision+':'+name+'\n' for name in names).encode())
        result = {}; offset = 0
        for name in names:
            end = stream.index(b'\n', offset); header = stream[offset:end].split(); size = int(header[2]); offset = end+1
            raw = stream[offset:offset+size]; result[name] = {'bytes': size, 'sha256': sha(raw)}; offset += size+1
        assert offset == len(stream)
        return result
    protected_go = blobs('HEAD'); assert protected_go == blobs(PARENT)
    report['unchanged_Go_files'] = protected_go
    report['unchanged_dependencies'] = {}
    manifests = [name for name in git('ls-files').decode().splitlines() if name.endswith('Cargo.toml') or name in ('Cargo.lock', 'rust-toolchain.toml')]
    for name in manifests:
        raw = (ROOT/name).read_bytes() if (ROOT/name).exists() else git('show', 'HEAD:' + name)
        assert raw == parent(name)
        report['unchanged_dependencies'][name] = {'bytes': len(raw), 'sha256': sha(raw)}
    for name in ['cases.py', 'fixtures.py', 'compare.py', 'output_cases.py', 'library_denied.py']:
        path = 'scripts/skills-native-oracle/' + name; raw = (ROOT/path).read_bytes()
        assert raw == parent(path), path
    sys.path.insert(0, str(ROOT/'scripts/skills-native-oracle'))
    run = module(ROOT/'scripts/skills-native-oracle/run.py', 'prepared_byte_run')
    byte = sys.modules['byte_cases']
    projections = {}
    old_platform = os.name
    for platform_name in ['posix', 'nt']:
        try:
            os.name = platform_name
            env = vars(run).copy(); env['root'] = Path('/owned') if platform_name == 'posix' else root
            # Build the POSIX Path before setting os.name to nt; there is no FS
            # operation in any case generator on this synthetic path.
            old = case_projection(parent('scripts/skills-native-oracle/run.py'), env.copy())
            new = case_projection((ROOT/'scripts/skills-native-oracle/run.py').read_bytes(), env.copy())
            old_ids = [row[0] for row in old]; new_ids = [row[0] for row in new]
            assert new[:len(old)] == old and len(set(new_ids)) == len(new_ids)
            total_old = old_ids + run.denied_ids() + run.output_ids()
            total_new = new_ids + run.denied_ids() + run.output_ids()
            projections[platform_name] = {'old_ids': total_old, 'new_ids': total_new,
                                         'old_total': len(total_old), 'new_total': len(total_new),
                                         'additive_cases': len(new)-len(old)}
        finally: os.name = old_platform
    report['prepared_case_projections'] = projections
    # Exact first-review request body inputs are supplied after the unchanged
    # initialization/catalog prefix, in both wire modes.
    rows = byte.cases(root, run.incoming)
    by_id = {row[0]: row for row in rows}
    for label in ('meta-array-read', 'meta-array-write', 'meta-overflow-read', 'name-type-then-valid', 'name-valid-then-null'):
        for mode in ('line', 'framed'):
            original = (REVIEW/f'outer-{label}.{ "jsonl" if mode == "line" else "framed"}').read_bytes()
            data = by_id[f'byte-outer-{label}-{mode}'][2]
            assert data.endswith(original), (label, mode)
    report['original_outer_raw_inputs_preserved_and_prepared'] = 10
    for path in REVIEW.glob('request-*'):
        if path.suffix not in ('.jsonl', '.framed'): continue
        original = path.read_bytes()
        assert any(data is not None and data.endswith(original) for _, _, data, _ in rows), path.name
    report['original_byte_transport_inputs_prepared'] = 8
    for key, filename in [('ff','ff'), ('e282','e282'), ('c0af','c0af'), ('ordinary',b'ordinary UTF8'.hex())]:
        assert byte.original_document(key) == (REVIEW/f'body-{filename}.SKILL.md').read_bytes()
    report['original_full_document_bytes_prepared'] = 4
    # These fixtures are generator outputs only; no runtime variant is called.
    raw_inputs = {name: {'stdin_hex': data.hex(), 'sha256': sha(data), 'bytes': len(data), 'variant': selected}
                  for name, _, data, selected in rows if data is not None}
    (OUT/'prepared-inputs.json').write_text(json.dumps({'kind':'PREPARED_NOT_EXECUTED', 'inputs':raw_inputs}, indent=2)+'\n')
    report['prepared_additive_raw_MCP_inputs'] = len(raw_inputs)
    report['original_controls_retained'] = ['argv-target', 'argv-limit', 'mcp-type']
    report['additive_actual_child_controls_prepared'] = ['byte-outer-meta-control', 'byte-body-admission-control']
    # No helper/controls/fixture/comparator body was changed for old plans.
    old_tree = ast.parse(parent('scripts/skills-native-oracle/run.py')); new_tree = ast.parse((ROOT/'scripts/skills-native-oracle/run.py').read_bytes())
    for name in ('sha','terminate','invoke','incoming','source_map','actual_control'):
        old = next(n for n in old_tree.body if isinstance(n, ast.FunctionDef) and n.name == name)
        new = next(n for n in new_tree.body if isinstance(n, ast.FunctionDef) and n.name == name)
        assert ast.dump(old, include_attributes=False) == ast.dump(new, include_attributes=False), name
    # Durable pair registration and raw child input precede all comparison.
    loop = (ROOT/'scripts/skills-native-oracle/run.py').read_text()
    assert loop.index('report["results"].append(pair)') < loop.index('invoke(binaries[flavor]') < loop.index('pair["matched"] = matched')
    python = list((ROOT/'scripts/skills-native-oracle').glob('*.py')) + [Path(__file__)]
    for path in python: ast.parse(path.read_bytes(), filename=str(path))
    report['python_AST_parsed_files'] = len(python)
    changed = git('diff', '--name-only', PARENT).decode().splitlines() + git('ls-files','--others','--exclude-standard').decode().splitlines()
    report['changed_Rust_line_counts'] = {name:len((ROOT/name).read_text().splitlines()) for name in sorted(set(changed)) if name.endswith('.rs')}
    assert all(count < 400 for count in report['changed_Rust_line_counts'].values())
    source_names = git('ls-files', 'rust/symbrain-skills','rust/symbrain-cli','rust/symbrain-gateway','rust/symbrain-mcp','scripts/skills-native-oracle','Cargo.lock','migration/contract-matrix.csv','.github/workflows/skills-native.yml').decode().splitlines()
    source_names += [name for name in changed if name.startswith(('rust/','scripts/skills-native-oracle/'))]
    report['full_candidate_source_graph'] = {name:{'bytes':(ROOT/name).stat().st_size,'sha256':sha((ROOT/name).read_bytes())} for name in sorted(set(source_names))}
    report['status'] = 'passed_source_checks_only'
    (OUT/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({key: value for key,value in report.items() if key not in ('unchanged_Go_files','full_candidate_source_graph','prepared_case_projections','unchanged_dependencies','changed_Rust_line_counts')}))
    print(json.dumps({name:{key:value for key,value in row.items() if key.endswith('total') or key=='additive_cases'} for name,row in projections.items()}))


root = Path('/owned')
if __name__ == '__main__': main()
