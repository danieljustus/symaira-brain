"""Reproduce source/fixture checks in a fresh OUTPUT directory; no products."""
import ast
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[4]
SCRIPTS = ROOT/'scripts/guard-standalone-oracle'
BASE = 'e13f4caa07a4aecff5ab54a4ab57e2e248374438'


def main():
    output = Path(sys.argv[1]).resolve()
    output.mkdir(parents=True, exist_ok=False)
    owned = output/'owned-tmp'; owned.mkdir()
    env = os.environ.copy()
    env.update(TMPDIR=str(owned), PYTHONDONTWRITEBYTECODE='1')
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(SCRIPTS))
    import config_paths
    import raw_paths
    import kernel_admission
    unchanged = {}
    for filename, names in [('raw_paths.py', ['compare']), ('config_paths.py',
                            ['cases', 'compare', 'control', 'expected_owner', 'prepare_discovery_parents'])]:
        before = ast.parse(subprocess.check_output(['git', 'show', BASE+':scripts/guard-standalone-oracle/'+filename], cwd=ROOT))
        after = ast.parse((SCRIPTS/filename).read_bytes())
        for name in names:
            left = next(n for n in before.body if isinstance(n, ast.FunctionDef) and n.name == name)
            right = next(n for n in after.body if isinstance(n, ast.FunctionDef) and n.name == name)
            assert ast.dump(left, include_attributes=False) == ast.dump(right, include_attributes=False)
            unchanged[filename+':'+name] = True
    original_raw = ast.parse(subprocess.check_output(['git', 'show', BASE+':scripts/guard-standalone-oracle/raw_paths.py'], cwd=ROOT))
    current_raw = ast.parse((SCRIPTS/'raw_paths.py').read_bytes())
    for name in ['KINDS', 'COMPONENTS', 'UNIX_COMPONENTS']:
        def assignment(tree):
            return next(n for n in tree.body if isinstance(n, ast.Assign) and any(isinstance(t, ast.Name) and t.id == name for t in n.targets))
        assert ast.dump(assignment(original_raw), include_attributes=False) == ast.dump(assignment(current_raw), include_attributes=False)
    for name in ['compare', 'controls']:
        def assertions(tree):
            function = next(n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == name)
            return [ast.dump(n, include_attributes=False) for n in ast.walk(function) if isinstance(n, ast.Assert)]
        assert assertions(original_raw) == assertions(current_raw)
    assert len(config_paths.cases()) == (23 if os.name == 'nt' else 25)
    assert len(raw_paths.KINDS)*(len(raw_paths.COMPONENTS)+len(raw_paths.UNIX_COMPONENTS)) == 63
    with tempfile.TemporaryDirectory(dir=owned) as directory:
        components = list(raw_paths.COMPONENTS)
        if os.name != 'nt':
            components += raw_paths.UNIX_COMPONENTS+[b'owned-\xe2\x82<&>', b'owned-\xe2\x82<&>-home']
        probes = [kernel_admission.probe(component, directory) for component in components]
    (output/'kernel-probes.json').write_text(json.dumps(probes, indent=2)+'\n')
    tests = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(SCRIPTS),
                            '-p', 'test_config_path*.py', '-v'], cwd=ROOT, env=env, capture_output=True)
    (output/'portable.stdout').write_bytes(tests.stdout)
    (output/'portable.stderr').write_bytes(tests.stderr)
    assert tests.returncode == 0
    mutants = []
    for name, original, replacement in [
        ('drop-accounting', 'assert set(done + missing) == set(expected),', 'assert True,'),
        ('broad-errno', "if system == 'darwin' and invalid_utf8(component) and error.errno == 92:", "if system == 'darwin':")]:
        with tempfile.TemporaryDirectory(dir=owned) as directory:
            stage = Path(directory)
            source = (SCRIPTS/'kernel_admission.py').read_text()
            assert source.count(original) == 1
            (stage/'kernel_admission.py').write_text(source.replace(original, replacement))
            shutil.copy2(SCRIPTS/'test_config_path_admission.py', stage/'test_config_path_admission.py')
            child = subprocess.run([sys.executable, '-m', 'unittest', 'discover', '-s', str(stage),
                                    '-p', 'test_config_path_admission.py', '-v'], env=env, capture_output=True)
            (output/(name+'.stdout')).write_bytes(child.stdout)
            (output/(name+'.stderr')).write_bytes(child.stderr)
            assert child.returncode == 1 and b'FAILED (failures=' in child.stderr
            mutants.append(dict(id=name, rejected=True, exit_code=child.returncode,
                                source_sha256=hashlib.sha256((stage/'kernel_admission.py').read_bytes()).hexdigest()))
    protected = []
    for line in subprocess.check_output(['git', 'ls-tree', '-r', BASE], cwd=ROOT, text=True).splitlines():
        prefix, path = line.split('\t'); mode, kind, blob = prefix.split()
        if path.startswith(('rust/', 'guard/', 'cmd/', 'internal/', '.github/')) or path in ['Cargo.lock', 'go.mod', 'go.sum']:
            assert subprocess.check_output(['git', 'rev-parse', ':'+path], cwd=ROOT, text=True).strip() == blob
            protected.append(dict(path=path, mode=mode, blob=blob))
    for path in SCRIPTS.glob('*.py'):
        ast.parse(path.read_bytes())
    assert subprocess.run(['git', 'diff', '--check'], cwd=ROOT).returncode == 0
    receipt = dict(base=BASE, unchanged_criteria=unchanged, protected_files=protected,
                   tests_exit_code=tests.returncode, fixture_mutants=mutants,
                   actual_kernel_probes=len(probes), product_compiler_SDK_target_port_execution=False)
    (output/'verification.json').write_text(json.dumps(receipt, indent=2)+'\n')


if __name__ == '__main__':
    main()
