"""One exclusive target, verified package archives, actual accepted-parent copy."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

PARENT='8803847278546f3161eaaa84d2e65bc2eec594a2'
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
def build(repo,root,evidence,source,env,run):
    # Match the sibling argv oracle: keep incremental/debug metadata out of the
    # initial-versus-restored binary comparison.
    env = env.copy()
    # MSVC link.exe stamps wall-clock PE TimeDateStamp/PDB GUID; /Brepro keeps
    # the Windows rebuild byte-comparable. Inert on other targets.
    env.update(CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0',
               CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS='-C link-arg=/Brepro')
    suffix='.exe'if os.name=='nt'else'';target=Path(env.get('CARGO_TARGET_DIR',repo/'target')).resolve();live=target/'debug'/('symbrain'+suffix)
    run('go-cli-build',['go','-C',str(source),'build','-trimpath','-o',str(evidence/('go-usage'+suffix)),'./cmd/symbrain'],env)
    shutil.copyfile(repo/'scripts/usage-next-oracle/sentinel.go.txt',root/'sentinel.go')
    run('sentinel-build',['go','build','-trimpath','-o',str(evidence/('sentinel'+suffix)),str(root/'sentinel.go')],env)
    def clean(label):run(label,['python3',str(repo/'scripts/usage-copilot-kimi-oracle/clean_cli.py'),str(target),str(repo),str(evidence/(label+'.json')),'--with-usage'],env)
    clean('cli-initial-clean');run('candidate-initial-build',['cargo','build','--locked','-p','symbrain-cli'],env)
    candidate=evidence/('native-usage'+suffix);shutil.copy2(live,candidate);before=sha(candidate)
    parent=root/'parent-source';run('parent-source',['git','worktree','add','--quiet','--detach',str(parent),PARENT])
    try:
        assert subprocess.check_output(['git','rev-parse','HEAD'],cwd=parent,text=True, encoding='utf-8').strip()==PARENT
        assert not subprocess.check_output(['git','status','--porcelain'],cwd=parent)
        manifest={}
        for p in sorted((parent/'rust/symbrain-cli/src').rglob('*.rs'))+sorted((parent/'rust/symbrain-usage/src').rglob('*.rs'))+[parent/'Cargo.toml',parent/'Cargo.lock']:
            name=p.relative_to(parent).as_posix();assert p.read_bytes()==subprocess.check_output(['git','show',PARENT+':'+name],cwd=parent);manifest[name]=sha(p)
        clean('cli-parent-clean');run('parent-build',['cargo','build','--locked','-p','symbrain-cli','--manifest-path',str(parent/'Cargo.toml'),'--target-dir',str(target)],env)
        parent_binary=evidence/('parent-usage'+suffix);shutil.copy2(live,parent_binary)
        clean('cli-candidate-clean');run('candidate-final-build',['cargo','build','--locked','-p','symbrain-cli','--target-dir',str(target)],env)
        assert sha(live)==before and live.read_bytes()==candidate.read_bytes(),'actual clean candidate restored byte-identically'
        result=dict(parent_source=PARENT,parent_clean=True,parent_source_sha256=manifest,candidate_restored_byte_identical=True,binaries={kind:dict(path=str(p),sha256=sha(p),bytes=p.stat().st_size)for kind,p in [('go',evidence/('go-usage'+suffix)),('parent',parent_binary),('rust',candidate),('sentinel',evidence/('sentinel'+suffix))]},scope='One exclusive target; each CLI and Usage dependency variant package-cleaned only after lossless roundtrip archives and zero users. Immutable actual accepted parent and restored candidate copied. Never a second Cargo target.')
        (evidence/'cli-build.json').write_text(json.dumps(result,indent=2)+'\n', encoding='utf-8')
        return result
    finally:subprocess.run(['git','-C',str(repo),'worktree','remove','--force',str(parent)],check=True)
