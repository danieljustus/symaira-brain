#!/usr/bin/env python3
"""Actual ordered config warnings, buffered admission and original healthy states."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import replay

UNSUPPORTED = b'symguard doctor: unsupported native diagnostic state; no legacy fallback is available\n'


def cases():
    # Exact fourteen input bytes from the original Go-only positive inventory.
    original = [
        ('unknown-root', 'owned=1\n'),
        ('unknown-root-array', 'owned=[{nested=1},{nested=2}]\n'),
        ('unknown-root-table', '[owned]\nnested=1\n'),
        ('unknown-quoted-dot', '"owned.dot"=1\n'),
        ('unknown-unicode', '"owned\\u0085"=1\n'),
        ('unknown-proxy', '[proxy]\nowned=1\n'),
        ('unknown-audit', '[audit]\nowned=1\n'),
        ('unknown-sequence', '[sequence]\nowned=1\n'),
        ('unknown-spawn', '[spawn]\nowned=1\n'),
        ('unknown-rule', '[[rules]]\ndecision="allow"\nowned=1\n[rules.match]\nserver="owned"\n'),
        ('unknown-match', 'rules=[{decision="allow",match={server="owned",owned=1}}]\n'),
        ('unknown-remote', 'remote=[{name="owned",extra=1}]\n'),
        ('unknown-allowlist', 'spawn={allowlist=[{path="/owned/synthetic",extra=1}]}\n'),
        ('unknown-all-order', 'z=1\n[audit]\nlast=1\nfirst=2\n[proxy]\nunknown=3\n'),
    ]
    extra = [
        ('dotted', 'owned.child=1\n'),
        ('late-parent', '[owned.child]\nx=1\n[owned]\ny=2\n'),
        ('array-tables', '[[owned]]\nx=1\n[[owned]]\nx=2\n'),
        ('nested-array-tables', '[[owned]]\n[owned.child]\nx=1\n[[owned]]\n[owned.child]\nx=2\n'),
        ('inline-dotted', 'owned={a.b=1}\n'),
        ('inline-nested', 'owned={a={b=1}}\n'),
        ('nested-arrays', 'owned=[[{x=1}],[{x=2}]]\n'),
        ('nested-array-inline', 'owned={a=[{x=1},{x=2}]}\n'),
        ('unknown-implicit-parent', '[proxy.owned]\na=1\n'),
        ('known-inline-array-duplicates', 'remote=[{name="a",extra={a=1}},{name="b",extra={a=2}}]\n'),
        ('rule-duplicates', 'rules=[{decision="allow",match={server="a",extra=1}},{decision="allow",match={server="b",extra=2}}]\n'),
        ('allowlist-owned', 'spawn={allowlist=[{path=OWNED_PATH,extra={a=1}}]}\n'),
        ('quoted-control', '"owned\\u0001\\u007f\\n\\t"=1\n'),
        ('quoted-slash', '"owned\\\\\\\""=1\n'),
        ('empty-key', '""=1\n'),
        ('quoted-dot-nesting', '["owned.dot"."child.dot"]\n"last.dot"=1\n'),
        ('quoted-unicode', '"雪\\u0085\\u2028\\u2029<&>"=1\n'),
        ('comments', '# owned\nz=1 # owned\n\n[audit] # owned\nlast=1\n# owned\nfirst=2\n'),
        ('empty-unknown-table', '[owned]\n'),
        ('empty-unknown-inline', 'owned={}\n'),
        ('empty-unknown-array', 'owned=[]\n'),
        ('unknown-values', 'owned=[true,1.5,"a",1979-05-27T07:32:00Z]\n'),
        ('dynamic-defaults', '[defaults]\nowned="allow"\n'),
        ('dynamic-quoted-defaults', 'defaults={"owned.dot"="deny"}\n'),
        ('warning-semantic-sequence', 'owned=1\n[sequence]\nenabled=true\nthreshold=1\n'),
        ('warning-semantic-default', 'owned=1\ndefaults={a="bad"}\n'),
        ('warning-semantic-rule', 'owned=1\nrules=[{decision="bad",match={server="a"}}]\n'),
        ('warning-semantic-spawn', 'owned=1\nspawn={allowlist=[{path="relative"}]}\n'),
        ('warning-before-known-field', 'owned=1\nproxy={upstream="a"}\n'),
        ('warning-after-known-field', 'proxy={upstream="a"}\nowned=1\n'),
    ]
    gated = [
        ('type-error-before-warning', 'sequence={enabled="bad"}\nowned=1\n', 'type'),
        ('type-error-after-warning', 'owned=1\nsequence={enabled="bad"}\n', 'type'),
        ('deep-type-after-warning', 'owned=1\nremote=[{labels=[1]}]\n', 'type'),
        ('case-fold-root', 'ProXy={upstream="owned"}\nowned=1\n', 'alias'),
        ('case-fold-field', 'proxy={Upstream="owned"}\nowned=1\n', 'alias'),
        ('unicode-fold-field', 'rules=[{decision="allow",match={"ſerver"="a"}}]\nowned=1\n', 'alias'),
        ('warning-discovery-delegation', 'owned=1\n', 'discovery'),
        ('warning-map-order-delegation', 'owned=1\ndefaults={a="bad",b="worse"}\n', 'map-order'),
    ]
    rows = [dict(id=name, data=text.encode(), contract='parity', original=True) for name, text in original]
    rows += [dict(id=name, data=text.encode(), contract='parity') for name, text in extra]
    rows += [dict(id=name, data=text.encode(), contract='gated', gate=gate) for name, text, gate in gated]
    raw = [b'owned-\xe2\x82<&>', 'owned-雪\u2028\u2029<&>'.encode()]
    if os.name != 'nt':
        for index, component in enumerate(raw):
            rows.append(dict(id='raw-path-'+str(index), data=b'"owned.dot"=1\n', contract='parity', path_bytes=component))
    return rows


def metadata(root):
    return {str(path.relative_to(root)): (path.read_bytes().hex(), path.stat().st_mode,
            path.stat().st_mtime_ns, path.stat().st_size)
            for path in root.rglob('*') if path.is_file()}


def observe(binary, case, root, native):
    env = replay.setup(root, 'empty')
    data = case['data'].replace(b'OWNED_PATH', json.dumps(str(root/'owned-tool'), ensure_ascii=False).encode())
    component = os.fsdecode(case.get('path_bytes', b'config.toml'))
    config = root/'home/.config/symguard'/component
    config.parent.mkdir(parents=True, exist_ok=True)
    config.write_bytes(data)
    env['SYMGUARD_CONFIG'] = str(config)
    if case.get('gate') == 'discovery':
        (root/'home/.cursor/mcp.json').write_bytes(b'{bad')
    before = metadata(root)
    command = [str(binary), 'doctor']
    if binary.suffix == '.py':
        command.insert(0, sys.executable)
    started = time.time()
    process = subprocess.run(command, cwd=root/'project', env=env, capture_output=True, timeout=5)
    ended = time.time()
    assert metadata(root) == before, 'doctor changed private input bytes/metadata'
    files = replay.state_files(root, started, ended, False)
    if b'OWNED_PATH' in case['data']:
        entry = files[str(config.relative_to(root)).replace('\\', '/')]
        prefix = json.dumps(str(root), ensure_ascii=False)[1:-1].encode()
        entry['normalized_hex'] = data.replace(prefix, b'<root>', 1).hex()
    return dict(exit_code=process.returncode, stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex(),
                normalized_stdout_hex=replay.normalized_stream(process.stdout, root, ['doctor'], native, True).hex(),
                normalized_stderr_hex=replay.normalized_stream(process.stderr, root, ['doctor'], native, False).hex(),
                files=files, read_only=True, input_hex=data.hex())


def compare(case, go, native):
    if case['contract'] == 'gated':
        assert native['exit_code'] == 1 and not native['stdout_hex']
        assert bytes.fromhex(native['stderr_hex']) == UNSUPPORTED, 'warning leaked before admission'
        warning = b'config: warning: unknown key ' in bytes.fromhex(go['stderr_hex'])
        assert warning == (case['gate'] != 'type'), 'Go typed-decode/warning boundary changed'
        assert go['exit_code'] == (0 if case['gate'] == 'alias' else 1)
        return 'native-fail-closed-remaining-port'
    assert replay.comparable(go) == replay.comparable(native), 'ordered warning/report/exit/state differs'
    return 'matched'


def controls(go, native, root, selected):
    rows = []
    modes = [('drop-warning', 'unknown-root-array'), ('reorder-warnings', 'unknown-all-order'),
             ('deduplicate-warnings', 'unknown-root-array'), ('leak-type-warning', 'type-error-after-warning'),
             ('double-delegation-warning', 'warning-discovery-delegation')]
    for index, (mode, ident) in enumerate(modes):
        base = root/('control-'+str(index)); base.mkdir()
        wrapper = base/'mutant.py'
        wrapper.write_text("""import subprocess,sys
p=subprocess.run([NATIVE,*sys.argv[1:]],capture_output=True)
if MODE=='double-delegation-warning':
    assert p.returncode==1 and not p.stdout and p.stderr==UNSUPPORTED
    p=subprocess.run([GO,*sys.argv[1:]],capture_output=True)
    lines=p.stderr.splitlines(keepends=True)
    assert p.returncode==1 and len(lines)==1 and lines[0].startswith(b'config: warning: unknown key ')
    stderr=lines[0]+p.stderr
elif MODE=='leak-type-warning':
    assert p.returncode==1 and not p.stdout and p.stderr==UNSUPPORTED
    stderr=b'config: warning: unknown key "owned" leaked before decode\\n'+p.stderr
else:
    lines=p.stderr.splitlines(keepends=True)
    assert p.returncode==0 and len(lines)>=3 and all(x.startswith(b'config: warning: unknown key ') for x in lines)
    if MODE=='drop-warning':stderr=b''.join(lines[1:])
    elif MODE=='reorder-warnings':stderr=b''.join(reversed(lines))
    else:
        assert len(set(lines))<len(lines)
        stderr=b''.join(dict.fromkeys(lines))
    assert stderr!=p.stderr
sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(stderr);sys.exit(p.returncode)
""".replace('NATIVE', repr(str(native))).replace('UNSUPPORTED', repr(UNSUPPORTED))
            .replace('GO', repr(str(go))).replace('MODE', repr(mode)), encoding='utf-8')
        case = selected[ident]
        left = observe(go, case, base/'go', False)
        right = observe(wrapper, case, base/'native', mode != 'double-delegation-warning')
        assert right['exit_code'] == (1 if case['contract'] == 'gated' else 0), 'mutant failed incidentally'
        try:
            if mode == 'double-delegation-warning':
                assert right['stdout_hex'] and bytes.fromhex(right['stderr_hex']).count(b'config: warning: unknown key ') == 2
                assert replay.comparable(left) == replay.comparable(right), 'duplicate delegated warning'
            else:
                compare(case, left, right)
        except AssertionError as error:
            rows.append(dict(id=mode, rejected=True, diagnostic=str(error), go=left, mutated_native=right,
                             wrapper_sha256=replay.digest(wrapper.read_bytes())))
        else:
            raise AssertionError('accepted actual warning mutation: '+mode)
    return rows


def main():
    go, native, report = map(Path, sys.argv[1:])
    go, native = go.resolve(strict=True), native.resolve(strict=True)
    selected = cases(); assert len({c['id'] for c in selected}) == len(selected)
    rows = []
    with tempfile.TemporaryDirectory(prefix='guard770-warning-process-') as raw:
        root = Path(raw)
        for index, case in enumerate(selected):
            left = observe(go, case, root/str(index)/'go', False)
            right = observe(native, case, root/str(index)/'native', True)
            try: disposition = compare(case, left, right)
            except AssertionError as error: disposition = 'failed: '+str(error)
            rows.append(dict(id=case['id'], contract=case['contract'], original=case.get('original', False),
                             input_hex=case['data'].hex(), disposition=disposition, go=left, native=right))
        chosen = {c['id']:c for c in selected}
        repeated = [observe(go, chosen['unknown-all-order'], root/'repeat'/str(i), False) for i in range(10)]
        assert len({row['normalized_stderr_hex'] for row in repeated}) == 1, 'Go warnings reordered across runs'
        mutants = controls(go, native, root, chosen)
    output = dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'], cwd=replay.ROOT, text=True).strip(),
                  candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'], cwd=replay.ROOT)),
                  total=len(rows), matched=sum(r['disposition']=='matched' for r in rows),
                  gated=sum(r['disposition'].startswith('native-fail-closed') for r in rows), results=rows,
                  repeated_go_runs=repeated, repeated_distinct_stderr=1, controls=mutants,
                  unix_raw_paths_skipped=os.name == 'nt',
                  inapplicable_raw_cases=['raw-path-0', 'raw-path-1'] if os.name == 'nt' else [],
                  original14_input_sha256={c['id']:replay.digest(c['data']) for c in selected if c.get('original')},
                  binaries_sha256=dict(go=replay.digest(go.read_bytes()), native=replay.digest(native.read_bytes())),
                  source_sha256={str(p.relative_to(replay.ROOT)):replay.digest(p.read_bytes()) for p in [
                      Path(__file__), replay.ROOT/'rust/symguard-cli/src/doctor/config_warnings.rs',
                      replay.ROOT/'rust/symguard-cli/src/doctor/config.rs', replay.ROOT/'rust/symguard-cli/src/doctor/config_decode.rs',
                      replay.ROOT/'rust/symguard-cli/src/guard_doctor.rs', replay.ROOT/'rust/symguard-cli/src/lib.rs']},
                  limits=['typed/case-fold/malformed-discovery/map-order states stay delegated without native warnings',
                          'Linux raw Unix config paths are actual byte proofs; native Windows/macOS still required',
                          'No reclassification of original50 nondeterministic defaults; no full770/769 closure'])
    report.write_text(json.dumps(output, indent=2)+'\n', encoding='utf-8')
    failures = [r['id']+': '+r['disposition'] for r in rows if r['disposition'].startswith('failed')]
    assert not failures, failures
    print(f'actual warning cases{len(rows)}: {output["matched"]} matches/{output["gated"]} retained gates; repeats10/1order; actual mutants{len(mutants)} rejected')


if __name__ == '__main__':
    main()
