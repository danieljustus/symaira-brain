#!/usr/bin/env python3
"""Actual Go/native doctor representations and Unicode; remaining errors stay explicit."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import replay


def cases():
    tables = [
        ('defaults-inline', 'defaults={read="allow"}\n'),
        ('defaults-empty', 'defaults={}\n'),
        ('defaults-multiline', 'defaults={\nread="allow",# owned\nnetwork="ask",}\n'),
        ('defaults-invalid', 'defaults={read="bad"}\n'),
        ('defaults-quoted', 'defaults={"owned\\u0085"="bad\\u007f"}\n'),
        ('rules-empty', 'rules=[]\n'),
        ('rules-inline', 'rules=[{decision="allow",match={server="owned"}}]\n'),
        ('rules-dotted', 'rules=[{decision="allow",match.tool="owned"}]\n'),
        ('rules-multiple', 'rules=[{decision="allow",match={server="owned"}},{decision="deny",match={tool="owned"}},]\n'),
        ('rules-match-inline', '[[rules]]\ndecision="allow"\nmatch={capability="owned"}\n'),
        ('rules-invalid-decision', 'rules=[{decision="bad",match={server="owned"}}]\n'),
        ('rules-empty-match', 'rules=[{decision="allow",match={}}]\n'),
        ('rules-missing-decision', 'rules=[{match={server="owned"}}]\n'),
        ('rules-multiline-commands', 'rules=[{decision="allow",match={command_contains=[\n"owned",\n]}}]\n'),
        ('sequence-inline', 'sequence={enabled=true,threshold=3}\n'),
        ('sequence-zero', 'sequence={enabled=true,threshold=0}\n'),
        ('sequence-error', 'sequence={enabled=true,threshold=1}\n'),
        ('sequence-disabled-negative', 'sequence={enabled=false,threshold=-3}\n'),
        ('proxy-inline', 'proxy={upstream="owned"}\n'),
        ('proxy-new-escapes', 'proxy={upstream="\\e\\x41\\u0041"}\n'),
        ('audit-inline', 'audit={path="owned",encrypt=true,encrypt_age="owned"}\n'),
        ('remote-empty', 'remote=[]\n'),
        ('remote-inline', 'remote=[{name="owned",provider="owned",host="owned",trust_level="owned",allowed_servers=["owned"],labels=["owned"]}]\n'),
        ('remote-multiple', 'remote=[{}, {labels=[\n"owned",\n]}]\n'),
        ('spawn-empty-inline', 'spawn={allowlist=[]}\n'),
        ('spawn-owned-inline', 'spawn={allowlist=[{path=OWNED_PATH,argv_prefix=["owned"]}]}\n'),
        ('spawn-array-inline', '[spawn]\nallowlist=[{path=OWNED_PATH}]\n'),
        ('spawn-error-inline', 'spawn={allowlist=[{path="relative"}]}\n'),
        ('spawn-missing-path', 'spawn={allowlist=[{}]}\n'),
        ('all-empty-inline', 'defaults={}\nrules=[]\nproxy={}\naudit={}\nremote=[]\nsequence={}\nspawn={allowlist=[]}\n'),
    ]
    anchors = [
        ('known-high', b'{"content_hash":"\\ud800"}'),
        ('known-low', b'{"last_entry_hash":"\\udc00"}'),
        ('known-pair', b'{"content_hash":"\\ud800\\udc00"}'),
        ('known-highs', b'{"content_hash":"\\ud800\\ud800"}'),
        ('known-high-nonpair', b'{"content_hash":"\\ud800\\u0041"}'),
        ('known-lows', b'{"content_hash":"\\udc00\\udc00"}'),
        ('known-escaped', b'{"content_hash":"\\\\ud800"}'),
        ('known-malformed-escape', b'{"content_hash":"\\ud80x"}'),
        ('unknown-key-surrogate', b'{"\\ud800":1}'),
        ('unknown-value-surrogate', b'{"owned":"\\ud800"}'),
        ('unknown-deep-surrogate', b'{"owned":'+b'['*500+b'"\\ud800"'+b']'*500+b'}'),
        ('duplicate-repaired-then-null', b'{"content_hash":"\\ud800","content_hash":null}'),
        ('duplicate-null-then-repaired', b'{"content_hash":null,"content_hash":"\\ud800"}'),
        ('repaired-then-type', b'{"content_hash":"\\ud800","entry_count":true}'),
        ('type-then-repaired-key', b'{"entry_count":true,"\\ud800":1}'),
        ('numeric-string-surrogate', b'{"entry_count":"\\ud800"}'),
        ('numeric-error-before-repair', b'{"entry_count":9223372036854775808,"content_hash":"\\ud800"}'),
        ('numeric-error-after-repair', b'{"content_hash":"\\ud800","entry_count":1E+999}'),
        ('fold-escaped-key', b'{"la\\u017ft_entry_hash":"\\ud800"}'),
        ('literal-replacement', '{"content_hash":"\ufffd"}'.encode()),
    ]
    for index, bad in enumerate([b'\xff', b'\xe2\x82', b'\xf0\x80\x80', b'\xed\xa0\x80', b'\xc0\xaf']):
        anchors.extend([(f'bad-value-{index}', b'{"content_hash":"'+bad+b'"}'),
                        (f'bad-key-{index}', b'{"'+bad+b'":1}'),
                        (f'bad-after-type-{index}', b'{"entry_count":true,"content_hash":"'+bad+b'"}')])
    gated = [
        ('defaults-type', 'defaults={read=1}\n'),
        ('rules-type', 'rules=[1]\n'),
        ('rules-match-type', 'rules=[{decision="bad",match={command_contains=[1]}}]\n'),
        ('remote-type', 'remote=[1]\n'),
        ('remote-nested-type', 'remote=[{labels=[1]}]\n'),
        ('spawn-type', 'spawn={allowlist=[1]}\n'),
        ('spawn-prefix-type', 'spawn={allowlist=[{path=OWNED_PATH,argv_prefix=[1]}]}\n'),
        ('decode-before-validation', 'defaults={read="bad"}\nproxy={upstream=1}\n'),
        ('ordered-type-a', 'proxy={upstream=1}\naudit={encrypt="bad"}\n'),
        ('ordered-type-b', 'audit={encrypt="bad"}\nproxy={upstream=1}\n'),
        ('multiple-invalid-defaults', 'defaults={a="bad",b="worse"}\n'),
        ('unknown-root', 'owned=1\n'),
        ('unknown-inline', 'proxy={owned=1}\n'),
    ]
    return ([dict(id='table-'+name, kind='config', data=data.encode(), contract='parity') for name,data in tables]
            +[dict(id='anchor-'+name,kind='anchor',data=data,contract='parity') for name,data in anchors]
            +[dict(id='gated-'+name,kind='config',data=data.encode(),contract='native-fail-closed') for name,data in gated])


def observe(binary, case, root, native):
    env = replay.setup(root, 'empty')
    data = case['data'].replace(b'OWNED_PATH', json.dumps(str(root/'owned-tool'),ensure_ascii=False).encode())
    path = root/'home/.config/symguard/config.toml' if case['kind']=='config' else root/'data/symguard/audit.log.anchor'
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    if case['kind']=='anchor':
        path.with_suffix('').write_bytes(b'{}\n')
    command = [str(binary),'doctor']
    if binary.suffix=='.py':
        command.insert(0,sys.executable)
    start=time.time()
    process=subprocess.run(command,cwd=root/'project',env=env,capture_output=True,timeout=5)
    end=time.time()
    files=replay.state_files(root,start,end,False)
    if b'OWNED_PATH' in case['data']:
        name='home/.config/symguard/config.toml'
        record=files[name]
        assert bytes.fromhex(record['raw_hex'])==data
        prefix=json.dumps(str(root),ensure_ascii=False)[1:-1].encode()
        assert data.count(prefix)==1
        record['normalized_hex']=data.replace(prefix,b'<root>',1).hex()
    return dict(exit_code=process.returncode,stdout_hex=process.stdout.hex(),stderr_hex=process.stderr.hex(),
                normalized_stdout_hex=replay.normalized_stream(process.stdout,root,['doctor'],native,True).hex(),
                normalized_stderr_hex=replay.normalized_stream(process.stderr,root,['doctor'],native,False).hex(),
                files=files)


def compare(case,left,right):
    if case['contract']=='native-fail-closed':
        assert right['exit_code']==1 and not right['stdout_hex']
        assert bytes.fromhex(right['stderr_hex'])==b'symguard doctor: unsupported native diagnostic state; no legacy fallback is available\n'
        assert left['exit_code']==(0 if 'unknown' in case['id'] else 1)
        if 'unknown' in case['id']:
            assert bytes.fromhex(left['stderr_hex']).startswith(b'config: warning: unknown key ')
        return 'native-fail-closed-remaining-port'
    assert replay.comparable(left)==replay.comparable(right),'doctor stdout/stderr/exit/state differs'
    return 'matched'


def controls(go,rust,root,selected):
    modes=[('remove-inline-allowlist','table-spawn-owned-inline'),('gate-repaired-anchor','anchor-known-high'),('hide-error-after-repair','anchor-repaired-then-type')]
    rows=[]
    for index,(mode,case_id) in enumerate(modes):
        base=root/('control-'+str(index));base.mkdir()
        wrapper=base/'mutant.py'
        wrapper.write_text("""import re,subprocess,sys
p=subprocess.run([NATIVE,*sys.argv[1:]],input=sys.stdin.buffer.read(),capture_output=True)
assert not p.stderr
raw=p.stdout
if MODE=='gate-repaired-anchor':
    assert p.returncode==0 and b'anchor present' in raw
    sys.stderr.buffer.write(b'symguard doctor: unsupported native diagnostic state; no legacy fallback is available\\n');sys.exit(1)
if MODE=='remove-inline-allowlist':
    assert p.returncode==0 and b'ok (1 entries)' in raw
    raw,count=re.subn(rb'(?m)^(  spawn allowlist +)ok \\(1 entries\\)$',lambda m:m.group(1)+'not configured (empty — deny by default)'.encode(),raw)
else:
    assert p.returncode==1
    raw,count=re.subn(rb'(?m)^(  audit log +)error: [^\\n]+$',rb'\\1ok (hash-chained, anchor present)',raw)
assert count==1
sys.stdout.buffer.write(raw);sys.exit(p.returncode)
""".replace('NATIVE',repr(str(rust))).replace('MODE',repr(mode)),encoding='utf-8')
        case=selected[case_id]
        left=observe(go,case,base/'go',False);right=observe(wrapper,case,base/'rust',True)
        assert right['exit_code']==(1 if mode!='remove-inline-allowlist' else 0), right
        if mode!='gate-repaired-anchor':
            assert not right['stderr_hex'] and right['stdout_hex']
        try:compare(case,left,right)
        except AssertionError:rows.append(dict(id=mode,rejected=True,go=left,mutated_native=right,wrapper_sha256=replay.digest(wrapper.read_bytes())))
        else:raise AssertionError('accepted doctor mutant: '+mode)
    return rows


def main():
    go,rust,report=map(Path,sys.argv[1:]);go=go.resolve(strict=True);rust=rust.resolve(strict=True)
    selected=cases();assert len({c['id'] for c in selected})==len(selected)
    rows=[]
    with tempfile.TemporaryDirectory(prefix='guard770-doctor-boundaries-') as raw:
        root=Path(raw)
        for index,case in enumerate(selected):
            pair={side:observe(binary,case,root/str(index)/side,native) for side,binary,native in [('go',go,False),('rust',rust,True)]}
            try:disposition=compare(case,pair['go'],pair['rust'])
            except AssertionError as error:disposition='failed: '+str(error)
            rows.append(dict(id=case['id'],kind=case['kind'],contract=case['contract'],input_hex=case['data'].hex(),disposition=disposition,**pair))
        mutations=controls(go,rust,root,{c['id']:c for c in selected})
    output=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=replay.ROOT,text=True).strip(),candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=replay.ROOT)),total=len(rows),matched=sum(r['disposition']=='matched' for r in rows),gated=sum(r['disposition'].startswith('native-fail-closed') for r in rows),results=rows,controls=mutations,runner_sha256=replay.digest(Path(__file__).read_bytes()),binaries_sha256=dict(go=replay.digest(go.read_bytes()),rust=replay.digest(rust.read_bytes())))
    report.write_text(json.dumps(output,indent=2)+'\n',encoding='utf-8')
    failures=[r['id']+':'+r['disposition'] for r in rows if r['disposition'].startswith('failed')]
    assert not failures,failures
    print(f'doctor additive cases{len(rows)}: {output["matched"]} matches/{output["gated"]} explicit gates; actual mutants{len(mutations)} rejected')


if __name__=='__main__':main()
