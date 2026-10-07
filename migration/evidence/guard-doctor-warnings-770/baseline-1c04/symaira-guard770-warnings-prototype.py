import pathlib,sys,json,tempfile,subprocess,base64
sys.path.insert(0,'/workspace/symaira-guard770-doctor-warnings/scripts/guard-standalone-oracle')
import replay,doctor_boundaries as d
G=pathlib.Path('/tmp/symaira-guard770-diagnostics-independent-go');N=pathlib.Path('/workspace/symaira-guard770-diagnostics/target/debug/symguard')
old=json.load(open('/tmp/symaira-brain769-positive-slice-probe.json'))
cases=[(c['id'],c['input_utf8']) for c in old['cases'] if 'input_utf8'in c]
cases += [('dotted','owned.child=1\n'),('late-parent','[owned.child]\nx=1\n[owned]\ny=2\n'),('array-tables','[[owned]]\nx=1\n[[owned]]\nx=2\n'),('inline-dotted','owned={a.b=1}\n'),('nested-arrays','owned=[[{x=1}],[{x=2}]]\n'),('inline-nested','owned={a={b=1}}\n'),('unknown-nested-known','[proxy.owned]\na=1\n'),('quoted-control','"owned\\u0001\\u007f\\n\\t"=1\n'),('known-inline-array-duplicates','remote=[{name="a",extra={a=1}},{name="b",extra={a=2}}]\n'),('implicit-case-alias','[ProXy.owned]\na=1\n')]
rows=[]
with tempfile.TemporaryDirectory() as td:
 root=pathlib.Path(td)
 for i,(name,text) in enumerate(cases):
  case=dict(id=name,kind='config',data=text.encode(),contract='parity');l=d.observe(G,case,root/str(i)/'go',False);r=d.observe(N,case,root/str(i)/'rust',True)
  match=replay.comparable(l)==replay.comparable(r)
  rows.append(dict(id=name,input=text,matched=match,go=l,rust=r))
  print(name,match,l['exit_code'],r['exit_code'],bytes.fromhex(l['stderr_hex']).replace(str(root/str(i)/'go').encode(),b'<root>'),bytes.fromhex(r['stderr_hex']).replace(str(root/str(i)/'rust').encode(),b'<root>'))
pathlib.Path('/tmp/symaira-guard770-warnings-prototype.json').write_text(json.dumps(rows,indent=2)+'\n')
