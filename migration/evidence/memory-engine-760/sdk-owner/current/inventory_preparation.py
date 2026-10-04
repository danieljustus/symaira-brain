"""Read-only SDK byte census; no SDK process or mutable cache input."""
import sys
sys.dont_write_bytecode = True
import hashlib
import json
from pathlib import Path
sys.path.insert(0, '/workspace/symaira-memory760-sdk-owner/scripts/memory-engine-oracle')
import sdk
ROOT = Path('/workspace/symaira-memory760-sdk-owner')
HERE = ROOT / 'scripts/memory-engine-oracle'
legacy = json.loads((HERE / 'trusted.json').read_bytes())['sdk_platforms']['linux-x86_64']
roots = {'go': Path('/workspace/toolchains/go1.26.7'), 'rust': Path('/home/agent/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu')}
plan = {'kind': 'memory760-whole-sdk-v1', 'platforms': {'linux-x86_64': {}}}
for role, root in roots.items():
    observed = sdk.inventory(root)
    old = {**legacy[role]['files'], **legacy[role].get('source_files', {})}
    for name, value in old.items():
        assert observed['files'][name]['sha256'] == value, name
    plan['platforms']['linux-x86_64'][role] = observed
    print(role, 'files', len(observed['files']), 'directories', len(observed['directories']), 'unchanged legacy', len(old))
path = HERE / 'sdk-inputs.json'
path.write_text(json.dumps(plan, sort_keys=True, indent=2) + '\n')
print('map', len(path.read_bytes()), hashlib.sha256(path.read_bytes()).hexdigest())
assert plan['platforms']['linux-x86_64']['go']['files']['go.env']
assert len([x for x in plan['platforms']['linux-x86_64']['go']['files'] if x.startswith('pkg/include/')]) == 5
