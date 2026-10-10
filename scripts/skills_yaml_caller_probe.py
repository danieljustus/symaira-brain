#!/usr/bin/env python3
"""Compare real Go/Rust status and dry-run APIs over separate cold/hot roots.

Only root-derived API paths/cache-key names are mapped. Every payload byte,
entry, mode, marker, digest, source and installed-state comparison remains exact.
"""
import argparse
import copy
import hashlib
import json
import os
from pathlib import Path
import stat
import subprocess


def snapshot(root):
    result = {}
    for path in sorted(root.rglob('*')):
        mode = path.lstat().st_mode
        assert not stat.S_ISLNK(mode), path
        kind = 'directory' if stat.S_ISDIR(mode) else 'file'
        assert kind == 'directory' or stat.S_ISREG(mode), path
        result[path.relative_to(root).as_posix()] = dict(kind=kind, mode=stat.S_IMODE(mode),
            hex='' if kind == 'directory' else path.read_bytes().hex())
    return result


def seed(root, policy):
    root.mkdir(parents=True, mode=0o755)
    source = root/'library/probe'
    source.mkdir(parents=True)
    header = (b'---\nname: probe\ndescription: "left\\n\\0right"\nplatforms: [macos, linux]\nmetadata:\n'
              b'    item10: ten\n    item2: two\n    item1: one\n'
              b'    floats: [1.0, 1e6, -0.0, 1e-10]\n    nested: [[1, 2], [], [{inner: true}]]\n'
              b'    quoted: "123"\n    bool: true\n    "null": null\n    tab: "left\\n\\tright"\n')
    if policy == 'approved':
        header += b'    "<<": ["<<", {"<<": "\\n\\nX"}]\n    "\\nkey": "\\n"\n'
    (source/'SKILL.md').write_bytes(header+b'---\nbody\x00\xff\r\n\xe2\x82\xe2\x80\xa8\xe2\x80\xa9---\n')
    (source/'SKILL.md').chmod(0o640)
    for path, content, mode in [('references/raw.md',b'raw\r\n\xe2\x80\xa8\xe2\x80\xa9\x00\n',0o600),
                                ('scripts/helper.sh',b'#!/bin/sh\nprintf source\n',0o751),
                                ('assets/raw.bin',b'\x00\xff\xe2\x82\r\n',0o640),
                                ('symskills.toml',b'[skill]\nallow_executable = true\n',0o644)]:
        p = source/path
        p.parent.mkdir(parents=True,exist_ok=True)
        p.write_bytes(content)
        p.chmod(mode)
    installed = root/'home/.config/opencode/skills/probe'
    installed.mkdir(parents=True)
    (installed/'SKILL.md').write_bytes(b'protected installed bytes\n')
    (installed/'SKILL.md').chmod(0o600)
    marker = dict(managed_by='symskills',name='probe',target='opencode',mode='copy',
                  source_hash='retained-old-hash',installed='2020-01-02T03:04:05Z')
    (installed/'.symskills.json').write_bytes(json.dumps(marker,sort_keys=True).encode()+b'\n')
    (root/'home/user-sentinel').write_bytes(b'protected user state\x00\n')
    (root/'base').mkdir()
    (root/'retained-render').mkdir()


def canonical_cache(root, state):
    source = str(root/'library/probe').encode()
    key = hashlib.sha256(b'status-render-v1\0'+source+b'\0opencode').hexdigest()
    mapped = {}
    if 'comparison-cache' in state:
        mapped['.'] = state['comparison-cache']
    for path, data in state.items():
        if not path.startswith('comparison-cache/'):
            continue
        suffix = path.removeprefix('comparison-cache/')
        suffix = suffix.replace(key, '<verified-root-key>')
        assert key not in suffix
        mapped[suffix] = data
    return mapped


def validate_cache(root, state):
    mapped = canonical_cache(root, state)
    tree = 'status-render/<verified-root-key>'
    expected = {'.', 'status-render', tree, tree+'.json',
                'status-render/.symskills-lock-<verified-root-key>'}
    relative = {'SKILL.md', '.symskills.json', 'assets', 'assets/raw.bin',
                'references', 'references/raw.md', 'scripts', 'scripts/helper.sh'}
    expected.update(tree+'/'+path for path in relative)
    assert set(mapped) == expected, 'cache inventory'
    marker = json.loads(bytes.fromhex(mapped[tree+'/.symskills.json']['hex']))
    manifest = marker['output_manifest']
    assert {entry['path'] for entry in manifest} == relative-{'.symskills.json'}, 'marker inventory'
    for entry in manifest:
        observed = mapped[tree+'/'+entry['path']]
        raw = bytes.fromhex(observed['hex'])
        assert entry['kind'] == ('dir' if observed['kind']=='directory' else 'file'), 'manifest kind'
        assert entry['mode'] == format(observed['mode'],'04o'), 'manifest mode'
        assert entry['size'] == len(raw), 'manifest size'
        assert entry['sha256'] == ('' if entry['kind']=='dir' else hashlib.sha256(raw).hexdigest()), 'manifest hash'
    assert hashlib.sha256(json.dumps(manifest,separators=(',',':')).encode()).hexdigest() == marker['output_digest'], 'output digest'
    sidecar = json.loads(bytes.fromhex(mapped[tree+'.json']['hex']))
    source = root/'library/probe'
    fingerprint = hashlib.sha256()
    for path in sorted(p for p in source.rglob('*') if p.is_file()):
        rel = path.relative_to(source).as_posix()
        mode = '' if rel=='SKILL.md' else format(stat.S_IMODE(path.stat().st_mode),'04o')
        fingerprint.update(rel.encode()+b'\0'+mode.encode()+b'\0'+path.read_bytes()+b'\0')
    assert sidecar == dict(fingerprint=fingerprint.hexdigest(),target='opencode',name='probe'), 'source sidecar'
    skill = bytes.fromhex(mapped[tree+'/SKILL.md']['hex'])
    assert hashlib.sha256(sidecar['fingerprint'].encode()+b'\0'+skill+b'\0opencode').hexdigest() == marker['source_hash'], 'source hash'
    return mapped


def cache_negative_controls(root, state):
    validate_cache(root,state)
    key = hashlib.sha256(b'status-render-v1\0'+str(root/'library/probe').encode()+b'\0opencode').hexdigest()
    tree = 'comparison-cache/status-render/'+key
    mutations = []
    changed = copy.deepcopy(state)
    changed[tree+'/SKILL.md']['hex'] += '00'
    mutations.append((changed,'manifest size'))
    changed = copy.deepcopy(state)
    marker = json.loads(bytes.fromhex(changed[tree+'/.symskills.json']['hex']))
    marker['output_digest'] = '0'*64
    changed[tree+'/.symskills.json']['hex'] = json.dumps(marker).encode().hex()
    mutations.append((changed,'output digest'))
    changed = copy.deepcopy(state)
    changed[tree+'/scripts/helper.sh']['mode'] = 0o400
    mutations.append((changed,'manifest mode'))
    changed = copy.deepcopy(state)
    changed[tree+'/extra'] = dict(kind='file',mode=0o600,hex='00')
    mutations.append((changed,'cache inventory'))
    for changed, reason in mutations:
        try:
            validate_cache(root,changed)
        except AssertionError as error:
            assert str(error) == reason, (reason,error)
        else:
            raise AssertionError('cache mutation accepted: '+reason)
    return len(mutations)


def execute(binary, language, root, setting, label, evidence):
    argv = [str(binary)] + (['-root',str(root),'-cache',setting] if language == 'go' else [str(root),setting])
    before = snapshot(root)
    env = os.environ.copy()
    env.update(HOME=str(root/'home'), XDG_CONFIG_HOME=str(root/'home/.config'),
               XDG_DATA_HOME=str(root/'home/.local/share'), XDG_CACHE_HOME=str(root/'home/.cache'),
               TMPDIR=str(evidence/'temp'), TZ='UTC', LC_ALL='C')
    Path(env['TMPDIR']).mkdir(mode=0o700,exist_ok=True)
    result = subprocess.run(argv,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=60)
    (evidence/(label+'.stdout')).write_bytes(result.stdout)
    (evidence/(label+'.stderr')).write_bytes(result.stderr)
    after = snapshot(root)
    (evidence/(label+'.state.json')).write_text(json.dumps(dict(before=before,after=after,exit=result.returncode,argv=argv),indent=2)+'\n')
    assert result.returncode == 0, (label,result.returncode,result.stderr)
    assert not result.stderr, (label,'stderr')
    report = json.loads(result.stdout)
    assert report['success'] is True and type(report['success']) is bool
    assert (report['platform'],report['arch']) == (('darwin','arm64') if language=='go' else ('macos','aarch64'))
    protected_before = {p:s for p,s in before.items() if p != 'comparison-cache' and not p.startswith('comparison-cache/')}
    protected_after = {p:s for p,s in after.items() if p != 'comparison-cache' and not p.startswith('comparison-cache/')}
    assert protected_before == protected_after, (label,'source/installed/user/base/render changed')
    if setting != 'explicit':
        assert before == after, (label,'default/empty cache wrote state')
    elif label.endswith('hot'):
        assert before == after, (label,'hot cache state changed')
    else:
        assert not any(p.startswith('comparison-cache') for p in before)
        assert any(p.endswith('/SKILL.md') and p.startswith('comparison-cache/') for p in after)
        assert any(p.endswith('/.symskills.json') and p.startswith('comparison-cache/') for p in after)
        assert any(p.endswith('.json') and '/status-render/' in p for p in after)
    if setting == 'explicit':
        validate_cache(root,after)
        if label.endswith('cold'):
            assert cache_negative_controls(root,after) == 4
    # Only known absolute API path fields are mapped, not arbitrary response data.
    for collection in ('status','dry_run'):
        assert len(report[collection]) == 1
        for row in report[collection]:
            path = Path(row['path'])
            assert path == root/'home/.config/opencode/skills/probe'
            row['path'] = 'ROOT/'+path.relative_to(root).as_posix()
    return dict(status=report['status'],dry_run=report['dry_run']), canonical_cache(root,after)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--go',type=Path,required=True)
    parser.add_argument('--rust',type=Path,required=True)
    parser.add_argument('--root',type=Path,required=True)
    parser.add_argument('--policy',choices=['legacy','approved'],default='legacy')
    args = parser.parse_args()
    assert args.root.is_absolute() and not args.root.exists()
    args.root.mkdir(parents=True)
    evidence = args.root/'evidence'
    evidence.mkdir()
    results = []
    for setting in ('default','empty','explicit'):
        roots = {language:args.root/(setting+'-'+language) for language in ('go','rust')}
        for root in roots.values(): seed(root,args.policy)
        for phase in ('cold','hot'):
            observed = {language:execute(getattr(args,language),language,root,setting,setting+'-'+language+'-'+phase,evidence)
                        for language,root in roots.items()}
            assert observed['go'] == observed['rust'], (setting,phase,'API/cache parity',observed)
            results.append(dict(id=setting+'-'+phase,success=True,cache_entries=len(observed['go'][1])))
    receipt = dict(policy=args.policy,cases=results,success=True,
                   binaries={language:hashlib.sha256(getattr(args,language).read_bytes()).hexdigest() for language in roots})
    (evidence/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print(json.dumps(receipt))

if __name__ == '__main__':
    main()
