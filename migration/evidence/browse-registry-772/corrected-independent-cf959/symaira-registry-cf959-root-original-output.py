import base64
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

old = json.loads(Path('/tmp/symaira-registry94-root-extra-output.json').read_text())
go = Path('/tmp/symaira-registry94-root-go-bin')
native = Path('/workspace/symaira-daemon772-registry/target/debug/symbrowse')
records = []
for original in old['records']:
    with tempfile.TemporaryDirectory(prefix='br-output-closure-') as tmp:
        root = Path(tmp)
        env = {key: os.environ[key] for key in ('SystemRoot', 'windir', 'ComSpec') if key in os.environ}
        env.update(HOME=tmp, USERPROFILE=tmp, XDG_CONFIG_HOME=tmp+'/config',
                   XDG_CACHE_HOME=tmp+'/cache', XDG_DATA_HOME=tmp+'/data',
                   XDG_STATE_HOME=tmp+'/state', XDG_RUNTIME_DIR=tmp+'/run', PATH='')
        observed = {}
        for label, binary in [('go', go), ('native', native)]:
            p = subprocess.run([str(binary), *original['args']], cwd=root, env=env,
                               capture_output=True, timeout=15)
            observed[label] = {'exit': p.returncode,
                              'stdout_base64': base64.b64encode(p.stdout).decode(),
                              'stderr_base64': base64.b64encode(p.stderr).decode()}
        assert not list(root.iterdir()), 'invalid session created files'
        records.append({'args': original['args'], **observed,
                        'match': observed['go'] == observed['native'],
                        'go_original_bytes_match': observed['go'] == original['go'],
                        'no_files_created': True})
assert all(x['match'] and x['go_original_bytes_match'] for x in records)
out = {'candidate_head': 'cf959b72515f7679ecc3cc5142e89918c30cb405',
       'records': records, 'matches': 12,
       'binary_sha256': {str(p): hashlib.sha256(p.read_bytes()).hexdigest() for p in [go, native]},
       'original_rejection_sha256': hashlib.sha256(Path('/tmp/symaira-registry94-root-extra-output.json').read_bytes()).hexdigest()}
Path('/tmp/symaira-registry-cf959-root-original-output.json').write_text(json.dumps(out, indent=2)+'\n')
print('12 original actual CLI pairs now match; Go bytes unchanged; no files created')
