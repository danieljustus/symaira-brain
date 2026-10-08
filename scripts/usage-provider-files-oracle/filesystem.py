"""Owned Unix filesystem subprocess probes; retain immutable Go FIFO timeout."""
import base64
import json
import os
import pathlib
import subprocess
import sys
import tempfile

output=pathlib.Path(sys.argv[3])
if os.name!='posix':
    output.write_text(json.dumps({'platform':sys.platform,'unix_only':True,'cases':[],'native_windows_capability_root':'requires native Windows constructor/CI proof'})+'\n', encoding='utf-8')
    sys.exit(0)
records=[]
with tempfile.TemporaryDirectory(prefix='codex-filesystem-768-')as scratch:
    root=pathlib.Path(scratch)
    for kind in ['symlink-outside','symlink-inside','directory','fifo']:
        home=root/kind
        store=home/'.codex'
        store.mkdir(parents=True)
        auth=store/'auth.json'
        if kind=='fifo':os.mkfifo(auth,0o600)
        elif kind=='directory':auth.mkdir()
        else:
            target=(home/'outside.json')if kind=='symlink-outside'else(store/'inside.json')
            target.write_text('{"access_token":"synthetic-owned-file"}', encoding='utf-8')
            auth.symlink_to(target)
        env={key:value for key,value in os.environ.items()if key in ['TMPDIR','TMP','TEMP']}
        env.update({'HOME':str(home),'USERPROFILE':str(home),'CODEX_HOME':str(store),'XDG_CONFIG_HOME':str(home/'config'),'XDG_DATA_HOME':str(home/'data'),'XDG_CACHE_HOME':str(home/'cache'),'PATH':'','ANTHROPIC_OAUTH_TOKEN':'env://USAGE_HERMES_ABSENT','SYMBRAIN_GO_BINARY':str(home/'absent-go')})
        observations=[]
        for binary in sys.argv[1:3]:
            try:
                result=subprocess.run([str(pathlib.Path(binary).resolve()),'usage','--json'],env=env,capture_output=True,timeout=1.5,check=False)
                observations.append({'exit':result.returncode,'stdout_b64':base64.b64encode(result.stdout).decode(),'stderr_b64':base64.b64encode(result.stderr).decode(),'timed_out':False})
            except subprocess.TimeoutExpired as error:
                observations.append({'timed_out':True,'stdout_b64':base64.b64encode(error.stdout or b'').decode(),'stderr_b64':base64.b64encode(error.stderr or b'').decode()})
        native=observations[1]
        assert not native['timed_out']and native['exit']==0,(kind,native)
        report=json.loads(base64.b64decode(native['stdout_b64']))
        assert next(row for row in report['providers']if row['id']=='codex')['configured']is False
        if kind=='fifo':assert observations[0]['timed_out'],'immutable Go FIFO behavior changed; reassess native-only contract'
        else:assert observations[0]==native,(kind,observations)
        records.append({'id':kind,'go':observations[0],'rust':native,'native_only_safety_contract':kind=='fifo'})
output.write_text(json.dumps({'platform':sys.platform,'unix_only':True,'cases':records},indent=2)+'\n', encoding='utf-8')
