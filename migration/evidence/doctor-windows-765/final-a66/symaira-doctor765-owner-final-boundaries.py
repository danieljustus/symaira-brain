"""Additional owned Doctor filesystem and managed-owner path boundaries."""
import base64, hashlib, importlib.util, json, os, pathlib, shutil, subprocess, sys, tempfile
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows')
GO=pathlib.Path('/workspace/oracles/symbrain-go-dcddcef0')
RUST=pathlib.Path('/workspace/symaira-setup765-source/target/debug/symbrain')
sys.path.insert(0,str(ROOT/'scripts/doctor-repair-oracle'))
spec=importlib.util.spec_from_file_location('doctor_second_boundaries',ROOT/'scripts/doctor-repair-oracle/replay.py')
d=importlib.util.module_from_spec(spec);sys.modules[spec.name]=d;spec.loader.exec_module(d)
original=d.configure
records=[]
with tempfile.TemporaryDirectory(prefix='doctor-second-boundary-tools-') as temporary,d.legacy.ReleaseFixtureServer() as url:
    temp=pathlib.Path(temporary);probe=d.build_probe(temp);d.legacy.RELEASE_BASE_URL=url
    for name,raw,fault,lexical in [('normal-bin-file',False,True,None),('raw-bin-file',True,True,None),('home-dot',False,False,'dot'),('home-parent',False,False,'parent'),('home-duplicate-slash',False,False,'slash'),('raw-home-control',True,False,None)]:
        def configure(case,root,env,probe):
            if raw: env['HOME']=str(root/os.fsdecode(b'home\xff\xe2\x82'))
            home=env['HOME']
            if lexical=='dot':env['HOME']=home+'/./'
            elif lexical=='parent':env['HOME']=home+'/../'+pathlib.Path(home).name
            elif lexical=='slash':env['HOME']=home+'//'
            original(case,root,env,probe)
            if fault:
                binary_dir=pathlib.Path(env['HOME'])/'.symaira/bin'
                shutil.rmtree(binary_dir)
                binary_dir.write_bytes(b'owned obstruction')
        d.configure=configure
        case=d.cases()[0].__class__(name,args=('doctor','--fix','--force-release'),verifier=True)
        record={'case':name,'raw_home':raw,'bin_file':fault,'lexical':lexical}
        for label,binary in [('go',GO),('rust',RUST)]:
            with tempfile.TemporaryDirectory(prefix='doctor-second-boundary-case-') as root:
                record[label]=d.observe(binary,case,pathlib.Path(root),probe,GO)
        record['differences']=[key for key in record['go']['contract'] if record['go']['contract'][key]!=record['rust']['contract'][key]]
        records.append(record)
    result=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=ROOT)),go_sha=d.digest(GO),rust_sha=d.digest(RUST),probe_sha=d.digest(probe),cases=records)
    pathlib.Path('/tmp/symaira-doctor765-owner-final-boundaries.json').write_bytes((json.dumps(result,indent=2)+'\n').encode())
    print(json.dumps([(r['case'],r['differences']) for r in records]))
