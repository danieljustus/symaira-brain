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
    args=('setup','--fix','--allow-unsigned')+(('--json',) if json_out else ())
    for fault,lexical in [(None,False),(None,True),('bin',True),('parent',True)]:
        extra.append(setup.legacy.Case(f'raw-install-{fault}-lexical-{lexical}-json-{json_out}',args,setup=fixture(fault,lexical),mutating=True))
setup.cases=lambda:extra
sys.argv=[__file__,str(GO),str(RUST),'/tmp/symaira-doctor-source-independent-third-fix-consumers-setup.json']
code=setup.main();print("Strict extra Setup gate exit",code)

