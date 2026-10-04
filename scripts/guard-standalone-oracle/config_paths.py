#!/usr/bin/env python3
"""Owned actual config-path selection, diagnostics, readonly state and corruption control."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import replay
import config_warnings
from config_path_journal import Journal


def cases():
    rows = []
    for family in ['xdg', 'home']:
        for spelling in ['plain', 'dot', 'dotdot', 'relative', 'relative-parent', 'symlink-dotdot']:
            rows.append(dict(id=family+'-'+spelling, family=family, spelling=spelling, contract='parity'))
        for state in ['semantic', 'typed', 'discovery']:
            rows.append(dict(id=family+'-symlink-'+state, family=family,
                             spelling='symlink-dotdot', state=state,
                             contract='gated' if state in ['typed', 'discovery'] else 'parity'))
    rows += [dict(id='explicit-'+family, family=family, spelling='symlink-dotdot',
                  explicit=True, contract='parity') for family in ['xdg', 'home']]
    rows += [dict(id='explicit-dotdot-filename', family='xdg', spelling='dotdot',
                  explicit=True, contract='parity'),
             dict(id='explicit-relative-owner', family='xdg', spelling='relative',
                  explicit=True, contract='parity'),
             dict(id='empty-xdg-home-fallback', family='home', spelling='dotdot', empty_xdg=True,
                  contract='parity')]
    if os.name != 'nt':
        rows += [dict(id=family+'-raw-dotdot', family=family, spelling='symlink-dotdot',
                      raw=True, contract='parity') for family in ['xdg', 'home']]
    return rows


def link_directory(link, target):
    if os.name == 'nt':
        # Owned directory junctions do not require Developer Mode/symlink privilege.
        process = subprocess.run([os.environ['COMSPEC'], '/c', 'mklink', '/J', str(link), str(target)],
                                 capture_output=True, timeout=5)
        assert process.returncode == 0, ('owned junction creation failed', process.stdout, process.stderr)
    else:
        link.symlink_to(target, target_is_directory=True)


def metadata(root):
    result = {}
    for path in root.rglob('*'):
        info = path.lstat()
        if path.is_symlink():
            result[str(path.relative_to(root))] = ('symlink', os.readlink(path))
        elif path.is_file():
            result[str(path.relative_to(root))] = (path.read_bytes().hex(), info.st_mode, info.st_mtime_ns)
    return result


def prepare_discovery_parents(root, lexical_base, physical_base):
    # Changing HOME/XDG also changes Discovery, including the control's
    # physical owner. Missing parent directories have a distinct Windows
    # ERROR_PATH_NOT_FOUND contract; healthy cases must exercise absent FILES.
    for base in [root/'home', lexical_base, physical_base, root/'unused-config']:
        for part in ['.config/hermes', '.cursor', '.vscode', '.config/opencode',
                     '.config/claude', 'Library/Application Support/Claude', 'claude']:
            (base/part).mkdir(parents=True, exist_ok=True)


def expected_owner(case, platform):
    # Frozen Go's actual Windows explicit-filename observations select the
    # lexical owner. Unix explicit filenames retain physical symlink traversal.
    physical = case.get('explicit') and case['spelling'] == 'symlink-dotdot' and platform != 'nt'
    return b'physical_owner' if physical else b'lexical_owner'


def observe(binary, case, root, native, journal=None):
    env = replay.setup(root, 'empty')
    env.pop('SYMGUARD_CONFIG', None)
    component = os.fsdecode(b'owned-\xe2\x82<&>') if case.get('raw') else 'cfg'
    leaf = component if case['family'] == 'xdg' else component+'-home'
    lexical_base = root/leaf
    physical_base = root/'physical'/leaf
    suffix = Path('symguard/config.toml') if case['family'] == 'xdg' else Path('.config/symguard/config.toml')
    lexical, physical = lexical_base/suffix, physical_base/suffix
    state = case.get('state', 'healthy')
    data = b'lexical_owner=1\n'
    if state == 'semantic': data += b'sequence={enabled=true,threshold=1}\n'
    if state == 'typed': data += b'sequence={enabled="bad"}\n'
    for path, text in [(lexical, data), (physical, b'physical_owner=1\n')]:
        path.parent.mkdir(parents=True, exist_ok=True); path.write_bytes(text)
    (root/'discard').mkdir()
    (root/'physical/anchor').mkdir()
    link_directory(root/'link', root/'physical/anchor')
    spelling = case['spelling']
    base = {'plain': str(lexical_base), 'dot': str(root)+'/./'+leaf,
            'dotdot': str(root)+'/discard/../'+leaf, 'relative': '../'+leaf,
            'relative-parent': '../discard/../'+leaf,
            'symlink-dotdot': str(root)+'/link/../'+leaf}[spelling]
    if case['family'] == 'xdg': env['XDG_CONFIG_HOME'] = base
    else:
        env.update(HOME=base, USERPROFILE=base); env.pop('XDG_CONFIG_HOME', None)
        if case.get('empty_xdg'): env['XDG_CONFIG_HOME'] = ''
    if case.get('explicit'):
        env['SYMGUARD_CONFIG'] = base+'/'+str(suffix)
        # Explicit config wins over a conflicting generated XDG base.
        env['XDG_CONFIG_HOME'] = str(root/'unused-config')
    prepare_discovery_parents(root, lexical_base, physical_base)
    if state == 'discovery':
        for home in [root/'home', lexical_base, physical_base]:
            cursor = home/'.cursor/mcp.json'; cursor.parent.mkdir(parents=True, exist_ok=True)
            cursor.write_bytes(b'{bad')
    before = metadata(root)
    command = [str(binary), 'doctor']
    if binary.suffix == '.py': command.insert(0, sys.executable)
    if journal:
        journal.event('child-start', case=case['id'], native=native, binary=str(binary),
                      argv=command, cwd=str(root/'project'), before=before,
                      environment_bytes={key:os.fsencode(value).hex() for key,value in env.items()},
                      lexical_input_hex=data.hex(), physical_input_hex=b'physical_owner=1\n'.hex())
    began = time.time()
    try:
        process = subprocess.run(command, cwd=root/'project', env=env, capture_output=True, timeout=5)
    except BaseException as error:
        if journal:
            journal.event('child-exception', case=case['id'], native=native,
                          exception_type=type(error).__name__, exception=str(error),
                          stdout_hex=None if getattr(error,'stdout',None) is None else error.stdout.hex(),
                          stderr_hex=None if getattr(error,'stderr',None) is None else error.stderr.hex(),
                          after=metadata(root))
        raise
    ended = time.time()
    if journal:
        journal.event('child-returned', case=case['id'], native=native,
                      exit_code=process.returncode, stdout_hex=process.stdout.hex(),
                      stderr_hex=process.stderr.hex(), began=began, ended=ended,
                      after=metadata(root))
    assert metadata(root) == before, 'doctor changed owned file bytes/mode/mtime or links'
    return dict(exit_code=process.returncode, stdout_hex=process.stdout.hex(), stderr_hex=process.stderr.hex(),
                normalized_stdout_hex=replay.normalized_stream(process.stdout, root, ['doctor'], native, True).hex(),
                normalized_stderr_hex=replay.normalized_stream(process.stderr, root, ['doctor'], native, False).hex(),
                files=replay.state_files(root, began, ended, False), readonly=True,
                input_state_metadata=before,
                config_environment_bytes={key:os.fsencode(value).hex() for key,value in env.items()
                    if key in ['HOME','USERPROFILE','XDG_CONFIG_HOME','SYMGUARD_CONFIG']},
                lexical_input_hex=data.hex(), physical_input_hex=b'physical_owner=1\n'.hex())


def compare(case, go, native):
    if case['contract'] == 'gated':
        assert go['exit_code'] == 1
        assert native['exit_code'] == 1 and not native['stdout_hex']
        assert bytes.fromhex(native['stderr_hex']) == config_warnings.UNSUPPORTED, 'warning before fallback'
        warning = b'config: warning: unknown key ' in bytes.fromhex(go['stderr_hex'])
        assert warning == (case['state'] == 'discovery')
        if case['state'] == 'discovery':
            # An earlier missing-parent error is not the malformed Cursor
            # contract this fixture intends to reach.
            stdout = bytes.fromhex(go['stdout_hex'])
            assert b'error: discovery: cursor (' in stdout, 'malformed Cursor not reached'
            assert b"[unsupported] parse JSON: invalid character 'b' looking for beginning of object key string" in stdout
        return 'native-fail-closed-remaining-port'
    assert replay.comparable(go) == replay.comparable(native), 'config owner/warning filename/report/exit/state differs'
    expected = expected_owner(case, os.name)
    assert expected in bytes.fromhex(native['stderr_hex']), 'wrong selected owner'
    assert native['exit_code'] == (1 if case.get('state') == 'semantic' else 0)
    return 'matched'


def control(go, native, root, journal=None):
    root.mkdir()
    wrapper = root/'canonicalize-mutant.py'
    wrapper.write_text("""import json,os,subprocess,sys
from pathlib import Path
old=os.environ['XDG_CONFIG_HOME'];assert '/link/../' in old
if os.name=='nt':
    # Windows realpath cleans link/.. before resolving the junction. Resolve
    # the owned link first so the mutant actually selects the physical owner.
    prefix,tail=old.rsplit('/link/../',1)
    resolved=os.path.realpath(prefix+'/link')
    os.environ['XDG_CONFIG_HOME']=os.path.normpath(os.path.join(resolved,'..',tail))
    assert (Path(os.environ['XDG_CONFIG_HOME'])/'symguard/config.toml').read_bytes()==b'physical_owner=1\\n'
else:
    os.environ['XDG_CONFIG_HOME']=os.path.realpath(old)
assert os.environ['XDG_CONFIG_HOME']!=old
p=subprocess.run([NATIVE,*sys.argv[1:]],capture_output=True)
Path(__file__).with_suffix('.child.json').write_bytes(json.dumps(dict(argv=[NATIVE,*sys.argv[1:]],cwd=os.getcwd(),environment_bytes={key:os.fsencode(value).hex() for key,value in os.environ.items()},exit_code=p.returncode,stdout_hex=p.stdout.hex(),stderr_hex=p.stderr.hex())).encode('utf-8'))
assert p.returncode==0 and b'physical_owner' in p.stderr
sys.stdout.buffer.write(p.stdout);sys.stderr.buffer.write(p.stderr);sys.exit(p.returncode)
""".replace('NATIVE',repr(str(native))))
    case=dict(id='canonicalize-owner-mutation',family='xdg',spelling='symlink-dotdot',state='semantic',contract='parity')
    if journal:
        journal.event('control-prepared', case=case, wrapper_source_hex=wrapper.read_bytes().hex(),
                      wrapper_sha256=replay.digest(wrapper.read_bytes()))
    left=observe(go,case,root/'go',False,journal)
    try:
        right=observe(wrapper,case,root/'native',True,journal)
    finally:
        child_report=wrapper.with_suffix('.child.json')
        if journal and child_report.exists():
            journal.event('control-native-child', report=json.loads(child_report.read_bytes()),
                          raw_report_hex=child_report.read_bytes().hex())
    if journal: journal.event('control-observed', go=left, mutated_native=right)
    assert right['exit_code']==0 and b'physical_owner' in bytes.fromhex(right['stderr_hex']), 'incidental mutant failure'
    try: compare(case,left,right)
    except AssertionError as error:
        return dict(id=case['id'],rejected=True,diagnostic=str(error),go=left,mutated_native=right,
                    wrapper_sha256=replay.digest(wrapper.read_bytes()))
    raise AssertionError('accepted real config owner corruption')


def run(go, native, report, journal):
    rows=[];selected=cases()
    with tempfile.TemporaryDirectory(prefix='guard770-config-paths-') as directory:
        root=Path(directory)
        for index,case in enumerate(selected):
            left=observe(go,case,root/str(index)/'go',False,journal)
            right=observe(native,case,root/str(index)/'native',True,journal)
            try: disposition=compare(case,left,right)
            except AssertionError as error: disposition='failed: '+str(error)
            rows.append(dict(**case,disposition=disposition,go=left,native=right))
            journal.event('pair-complete', observation=rows[-1])
        mutant=control(go,native,root/'control',journal)
        journal.event('control-complete', observation=mutant)
    output=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=replay.ROOT,text=True).strip(),
                candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=replay.ROOT)),
                observed_native_source_override=os.environ.get('OWNED_NATIVE_EXECUTABLE_SOURCE'),
                total=len(rows),matched=sum(row['disposition']=='matched' for row in rows),
                gated=sum(row['disposition'].startswith('native-fail-closed') for row in rows),results=rows,controls=[mutant],
                binaries_sha256=dict(go=replay.digest(go.read_bytes()),native=replay.digest(native.read_bytes())),
                source_sha256={str(path.relative_to(replay.ROOT)):replay.digest(path.read_bytes()) for path in [
                    Path(__file__),Path(__file__).with_name('config_path_journal.py'),replay.ROOT/'rust/symguard-cli/src/doctor/config.rs',
                    replay.ROOT/'rust/symguard-cli/src/guard_scan.rs',replay.ROOT/'rust/symguard-cli/src/guard_paths.rs',
                    replay.ROOT/'rust/symguard-cli/src/guard_path_windows.rs']},
                inapplicable_unix_raw_cases=['xdg-raw-dotdot','home-raw-dotdot'] if os.name=='nt' else [],
                limits=['Native macOS/Windows exact-source CI remains required','no full770/769 acceptance',
                        'Explicit overrides retain OS path interpretation','typed/discovery states remain delegated'])
    report.write_text(json.dumps(output,indent=2)+'\n')
    assert not [row for row in rows if row['disposition'].startswith('failed')], 'see complete raw config-path receipt'
    journal.complete(output)
    print(f'config paths{len(rows)}: {output["matched"]} full/{output["gated"]} gated; actual owner mutant rejected')


def main():
    go,native,report=map(Path,sys.argv[1:]);go=go.resolve(strict=True);native=native.resolve(strict=True)
    journal=Journal(report, dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=replay.ROOT,text=True).strip(),
                                candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=replay.ROOT)),
                                binaries_sha256=dict(go=replay.digest(go.read_bytes()),native=replay.digest(native.read_bytes())),
                                runner_sha256=replay.digest(Path(__file__).read_bytes()),
                                journal_sha256=replay.digest(Path(__file__).with_name('config_path_journal.py').read_bytes())))
    try:
        run(go,native,report,journal)
    except BaseException as error:
        journal.failed(error)
        raise


if __name__=='__main__': main()
