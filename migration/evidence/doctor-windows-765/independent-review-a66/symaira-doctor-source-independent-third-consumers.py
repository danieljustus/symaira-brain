"""Additional actual Setup consumers of raw lexical managed ownership/mkdir."""
import importlib.util, pathlib, os, sys, json, subprocess, tempfile, shutil
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows')
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain')
RAW_HOME=b'home\xff\xef\xbf\xbd\xe2\x82&<>\xe2\x80\xa8\xe2\x80\xa9'
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);sys.modules[name]=m;spec.loader.exec_module(m);return m
setup=load('third_setup_consumers',ROOT/'scripts/setup-repair-oracle/replay.py')
def fixture(fault, lexical):
    def configure(root,env):
        raw=os.fsdecode(RAW_HOME)
        home=root/raw;home.mkdir()
        env['HOME']=str(home)+('/../'+raw+'/./' if lexical else '')
        setup.legacy.setup_release_fixture(root,env)
        if fault:
            parent=home/'.symaira'
            if fault=='bin':parent.mkdir();(parent/'bin').write_bytes(b'owned leaf obstruction')
            else:parent.write_bytes(b'owned ancestor obstruction')
    return configure
extra=[]
for json_out in (False,True):
    args=('setup','--allow-unsigned')+(('--json',) if json_out else ())
    for fault,lexical in [(None,False),(None,True),('bin',True),('parent',True)]:
        extra.append(setup.legacy.Case(f'raw-install-{fault}-lexical-{lexical}-json-{json_out}',args,setup=fixture(fault,lexical),mutating=True))
setup.cases=lambda:extra
sys.argv=[__file__,str(GO),str(RUST),'/tmp/symaira-doctor-source-independent-third-consumers-setup.json']
code=setup.main();assert code==0,code

source=load('third_source_consumers',ROOT/'scripts/setup-source-oracle/replay.py')
original=source.configure;records=[]
with tempfile.TemporaryDirectory(prefix='third-source-tools-') as temporary:
    tmp=pathlib.Path(temporary);src=tmp/'fixture.go';src.write_bytes((ROOT/'scripts/setup-source-oracle/tool_fixture.go.txt').read_bytes());tool=tmp/'tool'
    subprocess.run(['go','build','-trimpath','-o',str(tool),str(src)],check=True)
    for json_out in (False,True):
        for fault in (None,'parent'):
            def configure(case,root,go,tool):
                env,args=original(case,root,go,tool)
                raw=os.fsdecode(RAW_HOME);home=root/raw;home.mkdir()
                env['HOME']=str(home)+'/../'+raw+'/./'
                if fault:(home/'.symaira').write_bytes(b'owned ancestor obstruction')
                return env,args
            source.configure=configure
            case=dict(source.cases()[0],name=f'raw-source-parent-{fault}-json-{json_out}',args=['setup','--from-source','<source>','--modules','browse']+(['--json'] if json_out else []))
            row={'case':case['name']}
            for label,binary in [('go',GO),('rust',RUST)]:
                with tempfile.TemporaryDirectory(prefix='third-source-raw-') as root:row[label]=source.observe(binary,case,pathlib.Path(root),GO,tool)
            row['differences']=[key for key in row['go']['contract'] if row['go']['contract'][key]!=row['rust']['contract'][key]]
            records.append(row)
    receipt=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),go_sha=source.digest(GO),rust_sha=source.digest(RUST),tool_sha=source.digest(tool),cases=records)
    pathlib.Path('/tmp/symaira-doctor-source-independent-third-consumers-source.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print('Source consumer results',[(row['case'],row['differences']) for row in records])
    assert all(not row['differences'] for row in records)

sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
doctor=load('third_doctor_consumers',ROOT/'scripts/doctor-repair-oracle/replay.py')
original_doctor=doctor.configure;records=[]
with tempfile.TemporaryDirectory(prefix='third-doctor-tools-') as temporary,doctor.legacy.ReleaseFixtureServer() as url:
    tmp=pathlib.Path(temporary);probe=doctor.build_probe(tmp);doctor.legacy.RELEASE_BASE_URL=url
    for name,symlink,fault in [('raw-home-lexical',False,False),('raw-home-symlink-owner',True,False),('raw-home-lexical-ancestor-file',False,True)]:
        def configure(case,root,env,probe):
            raw=os.fsdecode(RAW_HOME)
            if symlink:
                (root/'owner/nested').mkdir(parents=True);(root/'link').symlink_to('owner/nested',target_is_directory=True)
                env['HOME']=str(root/'link')+'/../'+raw
            else:
                (root/raw).mkdir();env['HOME']=str(root/raw)+'/../'+raw+'/./'
            original_doctor(case,root,env,probe)
            if fault:
                parent=pathlib.Path(env['HOME'])/'.symaira';shutil.rmtree(parent);parent.write_bytes(b'owned ancestor obstruction')
        doctor.configure=configure
        case=doctor.cases()[0].__class__(name,args=('doctor','--fix','--force-release'),verifier=True)
        row={'case':name}
        for label,binary in [('go',GO),('rust',RUST)]:
            with tempfile.TemporaryDirectory(prefix='third-doctor-raw-') as root:row[label]=doctor.observe(binary,case,pathlib.Path(root),probe,GO)
        row['differences']=[key for key in row['go']['contract'] if row['go']['contract'][key]!=row['rust']['contract'][key]];records.append(row)
    receipt=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),go_sha=doctor.digest(GO),rust_sha=doctor.digest(RUST),probe_sha=doctor.digest(probe),cases=records)
    pathlib.Path('/tmp/symaira-doctor-source-independent-third-consumers-doctor.json').write_text(json.dumps(receipt,indent=2)+'\n')
    print('Doctor consumer results',[(row['case'],row['differences']) for row in records])
    assert all(not row['differences'] for row in records)
