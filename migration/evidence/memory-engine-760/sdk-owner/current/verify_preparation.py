"""Source/archive-only verification. No SDK/compiler/product/target execution."""
import sys
sys.dont_write_bytecode = True
import ast
import gzip
import hashlib
import json
from pathlib import Path
import subprocess
ROOT = Path('/workspace/symaira-memory760-sdk-owner')
BASE = '7804d91b6f7b712a9d8fdfa68160bb8375b07a60'
OUT = Path('/workspace/review-proof/memory760-sdk-owner-preparation/current')
def sha(data): return hashlib.sha256(data).hexdigest()
old = json.loads((ROOT / 'migration/evidence/memory-engine-760/hermetic-build/retention.json').read_bytes())['original108_and_new17']
newroot = ROOT / 'migration/evidence/memory-engine-760/sdk-owner/original-review'
new = json.loads((newroot / 'retention.json').read_bytes())['files']
for group, base in ((old, ROOT), (new, newroot)):
    for row in group:
        packed = (base / row['retained']).read_bytes()
        assert sha(packed) == row['gzip_sha256'], row
        data = gzip.decompress(packed)
        assert len(data) == row['bytes'] and sha(data) == row['sha256'], row
functions = {}
for path in sorted((ROOT / 'scripts/memory-engine-oracle').glob('*.py')):
    tree = ast.parse(path.read_bytes())
    assert len(path.read_text().splitlines()) < 400, path
    functions[path.name] = len([n for n in ast.walk(tree) if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))])
sys.path.insert(0, str(ROOT / 'scripts/memory-engine-oracle'))
import binding
corpus = binding.cases()
assert len(corpus) == 60
assert binding.trusted()['controls'] == ['changed-input', 'zero-selected', 'output-obstruction']
protected = ['rust/symbrain-memory', 'scripts/memory-engine-oracle/cases.json', 'scripts/memory-engine-oracle/oracle_test.go.txt', 'scripts/memory-engine-oracle/trusted.json', 'scripts/memory-engine-oracle/dependency-inputs.json', 'scripts/memory-engine-oracle/replay.py', 'scripts/memory-engine-oracle/build.py', 'scripts/memory-engine-oracle/test_binding.py', 'scripts/memory-engine-oracle/test_ownership.py', 'Cargo.lock', 'Cargo.toml', 'go.mod', 'go.sum', 'migration/evidence/memory-engine-760/hermetic-build', 'migration/evidence/memory-engine-760/original', 'migration/evidence/memory-engine-760/runner-binding']
assert subprocess.check_output(['git', '-C', str(ROOT), 'diff', BASE, '--', *protected]) == b''
# Whole parent Git tree, including unmaterialized sparse files, remains intact.
changes = subprocess.check_output(['git', '-C', str(ROOT), 'diff', '--name-status', BASE], text=True).splitlines()
modified = [x.split('\t')[-1] for x in changes if x.startswith('M\t')]
assert set(modified) == {'scripts/memory-engine-oracle/binding.py', 'scripts/memory-engine-oracle/owner.py', 'scripts/memory-engine-oracle/README.md'}, modified
assert not any(x.startswith(('D\t', 'R', 'T\t')) for x in changes), changes
legacy = binding.trusted()['sdk_platforms']['linux-x86_64']
whole = json.loads((ROOT / 'scripts/memory-engine-oracle/sdk-inputs.json').read_bytes())['platforms']['linux-x86_64']
counts = {}
for role, plan in whole.items():
    known = {**legacy[role]['files'], **legacy[role].get('source_files', {})}
    for name, expected in known.items():
        assert plan['files'][name]['sha256'] == expected, name
    counts[role] = {'files': len(plan['files']), 'directories': len(plan['directories']), 'legacy_unchanged': len(known)}
headers = sorted(x for x in whole['go']['files'] if x.startswith('pkg/include/'))
assert len(headers) == 5 and 'go.env' in whole['go']['files']
result = {'kind': 'source-only-memory760-sdk-owner-preparation', 'parent': BASE, 'original125': len(old), 'new94_plus_receipt': len(new), 'sdk_inventory': counts, 'headers': headers, 'corpus': len(corpus), 'original_controls': binding.trusted()['controls'], 'python_ast_functions': functions, 'parent_changes': changes, 'actual_sdk_compiler_product_http_port_runs': 0, 'real_target_or_cache_access_or_allocation': 0}
(OUT / 'static-verification.json').write_text(json.dumps(result, indent=2) + '\n')
print(json.dumps({k:v for k,v in result.items() if k not in ('parent_changes','python_ast_functions')},indent=2))
