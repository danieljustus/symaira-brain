import base64
import hashlib
import json
from pathlib import Path
import sqlite3
import subprocess
import sys
import tempfile

REPO = Path('/workspace/symaira-memory758-cli-fixes')
sys.path.insert(0, str(REPO / 'scripts/memory-cli-oracle'))
import replay as h
GO = Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST = REPO / 'target/debug/symbrain'
records = []

def check(name, root, env, args, expected=None):
    before = h.snapshot(root)
    left = h.output(GO, args, root, env)
    middle = h.snapshot(root)
    right = h.output(RUST, args, root, env)
    unchanged = before == middle == h.snapshot(root)
    item = dict(name=name, match=left == right, successful=left['exit'] == 0,
                state_unchanged=unchanged, go=left, rust=right)
    if expected is not None:
        item['original_go_output_reproduced'] = left == expected
        assert item['original_go_output_reproduced'], item
    records.append(item)
    assert item['match'] and item['successful'] and unchanged, item

oldconfig = json.loads(Path('/tmp/symaira-memory758-cli-independent-config-extra.json').read_bytes())
for original in oldconfig['records']:
    text = original['name']
    with tempfile.TemporaryDirectory(prefix='memory-independent-config-extra-') as tmp:
        root = Path(tmp)
        env = h.isolated_env(root)
        h.seed(GO, root, env)
        config = root / 'config/symmemory/config.toml'
        config.parent.mkdir(parents=True)
        prefix = '' if text.startswith('database=') else 'database.path="configured.db"\n'
        config.write_bytes((prefix + text + '\n').encode())
        check('config:' + text, root, env, ['list'], original['go'])

oldpaths = json.loads(Path('/tmp/symaira-memory758-cli-independent-paths.json').read_bytes())
for original in oldpaths['records']:
    with tempfile.TemporaryDirectory(prefix='memory-independent-path-repro-') as tmp:
        root = Path(tmp)
        env = h.isolated_env(root)
        if original['name'] == 'relative-data-home':
            env['XDG_DATA_HOME'] = 'relative-data'
            pairs = [('go-home', root / '.local/share/symbrain/memory/default.db'),
                     ('rust-relative', root / 'relative-data/symbrain/memory/default.db')]
        else:
            pairs = [('go-xdg-legacy', root / 'data/symmemory/default.db'),
                     ('rust-home-legacy', root / '.local/share/symmemory/default.db')]
        for label, path in pairs:
            assert h.output(GO, ['list', '--db', str(path)], root, env)['exit'] == 0
            with sqlite3.connect(path) as db:
                db.execute("INSERT INTO memories(id,content,scope,kind,created_at,updated_at,metadata,embedding) VALUES(?,?,'global','reference','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC','{}',X'')", (label, label))
        config = root / 'config/symmemory/config.toml'
        config.parent.mkdir(parents=True)
        config.write_bytes(b'ollama.model="fixture-only"\n')
        check('original-path:' + original['name'], root, env, ['list', '--json'], original['go'])

oldrules = json.loads(Path('/tmp/symaira-memory758-cli-independent-rules.json').read_bytes())
with tempfile.TemporaryDirectory(prefix='memory-independent-rules-repro-') as tmp:
    root = Path(tmp)
    env = h.isolated_env(root)
    path = root / 'memory.db'
    assert h.output(GO, ['list', '--db', str(path)], root, env)['exit'] == 0
    with sqlite3.connect(path) as db:
        db.execute("INSERT INTO rules(id,content,scope,metadata,created_at,updated_at) VALUES('r1',?,'global','{}','2000-01-01 00:00:00 +0000 UTC','2000-01-01 00:00:00 +0000 UTC')", ('rule <private> & Unicode β\u2028',))
    config = root / 'config/symmemory/config.toml'
    config.parent.mkdir(parents=True)
    config.write_bytes(b'ollama.model="fixture-only"\n')
    check('original-populated-rules-html-js', root, env, ['rules', '--db', str(path), '--json'], oldrules['go'])

data = dict(candidate_revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=REPO,text=True).strip(),
            candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=REPO)),
            go_sha256=hashlib.sha256(GO.read_bytes()).hexdigest(),
            rust_sha256=hashlib.sha256(RUST.read_bytes()).hexdigest(),
            operator_home_used=False, fallback_available=False, records=records,
            passed=len(records))
Path('/tmp/symaira-memory758-cli-corrected-independent-extra.json').write_bytes((json.dumps(data, indent=2) + '\n').encode())
print(f'{len(records)}/{len(records)} independent extras matched; original Go outputs reproduced exactly; all tables/rows/blobs unchanged')
