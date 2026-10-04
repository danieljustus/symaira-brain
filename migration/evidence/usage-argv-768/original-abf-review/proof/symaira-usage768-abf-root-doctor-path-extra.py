import base64,hashlib,json,pathlib,random,subprocess
root=pathlib.Path('/tmp/symaira-usage768-abf-root-doctor-path')
def units(text):
 b=text.encode('utf-16-le','surrogatepass');return [int.from_bytes(b[i:i+2],'little') for i in range(0,len(b),2)]
def enc(v):return ','.join(f'{u:04x}'for u in v)
r=random.Random(76800)
prefixes=['','C:','C:\\','\\','/','\\\\host\\share\\','//host/share/','\\\\?\\C:\\','\\\\.\\UNC\\h\\s\\','\\??\\C:\\','\\\\.','\\\\?','\\??']
parts=['a','.','..','??','c:','雪','π','𐐀','\ud800','\udc00','\ud800\ud800','\udc00\udc00','\ud800x\udc00','\udfff:','\ud800:','a:','C:','CON','foo ','?','...']
pairs=[]
for prefix in prefixes:
 for unit in [0xd800,0xdc00,0xdfff,0xd801,0x2603]:
  pairs.append((units(prefix)+[unit]+units('/a/../owner'),units('credentials/kimi-code.json')))
for _ in range(6000):
 base=r.choice(prefixes)+r.choice(['/','\\']).join(r.choices(parts,k=r.randrange(0,8)))
 child=r.choice(prefixes[:5])+r.choice(['/','\\']).join(r.choices(parts,k=r.randrange(0,5)))
 pairs.append((units(base),units(child)))
data=('\n'.join(enc(a)+'|'+enc(b)for a,b in pairs)+'\n').encode()
(root/'input.txt').write_bytes(data)
observations=[]
for name in ['oracle','native']:
 process=subprocess.run([str(root/name)],input=data,capture_output=True,timeout=30)
 (root/(name+'.stdout')).write_bytes(process.stdout);(root/(name+'.stderr')).write_bytes(process.stderr)
 assert process.returncode==0 and not process.stderr
 rows=process.stdout.decode().splitlines();assert len(rows)==len(pairs)
 observations.append(rows)
mismatches=[dict(index=i,base_units=pairs[i][0],child_units=pairs[i][1],go=left,native=right)for i,(left,right)in enumerate(zip(*observations))if left!=right]
receipt=dict(cases=len(pairs),passed=len(pairs)-len(mismatches),failed=len(mismatches),mismatches=mismatches,scope=json.loads((root/'source.json').read_text())['scope'],input_sha256=hashlib.sha256(data).hexdigest(),source=json.loads((root/'source.json').read_text()),binary_sha256={name:hashlib.sha256((root/name).read_bytes()).hexdigest() for name in ['oracle','native']},raw_stdout_sha256={name:hashlib.sha256((root/(name+'.stdout')).read_bytes()).hexdigest()for name in ['oracle','native']})
(root/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(receipt['cases'],receipt['passed'],receipt['failed']);print(mismatches[:5])
