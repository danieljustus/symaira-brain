from pathlib import Path
import base64,datetime,hashlib,json,os,stat
def state(root,earliest,latest):
 result={}
 for p in sorted(root.rglob('*')):
  mode=stat.S_IMODE(p.lstat().st_mode);name=p.relative_to(root).as_posix()
  if p.is_symlink():result[name]=['link',mode,str(p.readlink())]
  elif p.is_dir():result[name]=['dir',mode]
  else:
   b=p.read_bytes().replace(os.fsencode(root),b'<root>')
   if name.endswith('.provenance.json'):
    v=json.loads(b);ts=v['built_at'];t=datetime.datetime.fromisoformat(ts.replace('Z','+00:00')).timestamp();assert ts.endswith('Z')and earliest-1<=t<=latest+1
    b=b.replace(ts.encode(),b'<validated-install-timestamp>')
   result[name]=['file',mode,len(b),hashlib.sha256(b).hexdigest(),base64.b64encode(b).decode()if len(b)<4096 else None]
 return result
