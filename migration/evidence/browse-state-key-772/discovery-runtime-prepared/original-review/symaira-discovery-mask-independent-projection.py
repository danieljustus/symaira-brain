#!/usr/bin/env python3
"""Source projections only: no actual Go/Rust/provider execution or target."""
from pathlib import Path
import hashlib,itertools,json
ROOT=Path('/workspace/symaira-daemon772-state-key-discovery-mask')
SDK=Path('/workspace/toolchains/go1.26.7')
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
# SDK internal/bisect.New transliteration: list stores mask AND bits; even a
# mask0 rule need not match, because its retained bits may be nonzero.
def sdk_new(pattern):
 if not pattern:return None
 p=pattern;quiet=False;enable=True
 if p.startswith('q'):quiet=True;p=p[1:]
 while p.startswith('v'):quiet=False;p=p[1:]
 while p.startswith('!'):enable=not enable;p=p[1:]
 if not p:return None
 if p=='n':enable=not enable;p='y'
 result=True;bits=0;start=0;width=1;conditions=[]
 for i,c in enumerate(p+'-'):
  if i==start and width==1 and c=='x':start=i+1;width=4;continue
  if c in '01' or width==4 and c in '23456789abcdefABCDEF':bits=((bits<<width)|int(c,16))&((1<<64)-1)
  elif c=='y':
   if i+1<len(p) and p[i+1] in '01':return None
   bits=0
  elif c in '+-':
   if c=='+' and not result:return None
   if i>0:
    count=(i-start)*width
    if not 0<count<=64:return None
    if p[start]=='y':count=0
    conditions.append(((1<<count)-1,bits,result))
   elif c=='-':conditions.append((0,0,True))
   bits=0;result=c=='+';start=i+1;width=1
  else:return None
 return quiet,enable,conditions
# Candidate godebug.rs parse/Tree::assign transliteration, retaining tree
# branch collapse rather than sampling a guessed Go stack hash.
def assign(tree,bits,width,result):
 if width<64 and bits>>width:return tree
 if width==0:return result
 if isinstance(tree,bool):tree=[tree,tree]
 branch=bits&1;tree[branch]=assign(tree[branch],bits>>1,width-1,result)
 if isinstance(tree[0],bool) and tree[0]==tree[1]:return tree[0]
 return tree

def native_parse(pattern):
 if not pattern:return None
 p=pattern;quiet=False;enable=True
 if p[0]=='q':quiet=True;p=p[1:]
 while p.startswith('v'):quiet=False;p=p[1:]
 while p.startswith('!'):enable=not enable;p=p[1:]
 if not p:return None
 if p=='n':enable=not enable;p='y'
 result=True;bits=0;start=0;width=1;tree=False
 for i,c in enumerate(p+'-'):
  if i==start and width==1 and c=='x':start=i+1;width=4;continue
  if c in '0123456789' and (width==4 or c<='1'):bits=((bits<<width)|int(c))&((1<<64)-1)
  elif c in 'abcdefABCDEF' and width==4:bits=((bits<<4)|int(c,16))&((1<<64)-1)
  elif c=='y':
   if i+1<len(p) and p[i+1] in '01':return None
   bits=0
  elif c in '+-':
   if c=='+' and not result:return None
   if i>0:
    count=(i-start)*width
    if not 0<count<=64:return None
    if p[start]=='y':count=0
    tree=assign(tree,bits,count,result)
   elif c=='-':tree=assign(tree,0,0,True)
   bits=0;result=c=='+';start=i+1;width=1
  else:return None
 return quiet,enable,tree

def sdk_result(pattern):
 m=sdk_new(pattern)
 if m is None:return dict(disposition='nil matcher',allow_relative=True,conditions=[])
 quiet,enable,conditions=m
 effective=[c for c in conditions if c[1]&~c[0]==0]
 # Test all suffix equivalence classes only when width bounded; our focused
 # corpus has at most16 bits. No SDK stack identity is invented.
 width=max((mask.bit_length() for mask,bits,result in effective),default=0)
 assert width<=16
 values=set()
 for value in range(1<<width):
  matched=next((result for mask,bits,result in reversed(effective) if value&mask==bits),False)
  values.add((matched==enable,not quiet and matched))
 if len(values)==1:
  enabled,printing=next(iter(values))
  if not printing:return dict(disposition='universal quiet/nonprinting',allow_relative=enabled,conditions=conditions)
 return dict(disposition='conditional or reporting',allow_relative=None,conditions=conditions)

def native_result(pattern):
 m=native_parse(pattern)
 if m is None:return dict(disposition='nil matcher',allow_relative=True)
 quiet,enable,tree=m
 if isinstance(tree,bool) and (quiet or not tree):return dict(disposition='leaf',allow_relative=tree==enable)
 return dict(disposition='explicit unsupported',allow_relative=None)


import gzip,random
folder=ROOT/'migration/evidence/browse-state-key-772/discovery-mask-prepared/actual-sdk-only'
retention=json.loads((folder/'retention.json').read_bytes());raws={}
for row in retention['files']:
 data=(folder/row['retained']).read_bytes();raw=gzip.decompress(data)
 assert hashlib.sha256(data).hexdigest()==row['gzip_sha256']
 assert hashlib.sha256(raw).hexdigest()==row['sha256'] and len(raw)==row['bytes']
 raws[row['original']]=raw
get=lambda suffix:next(raw for path,raw in raws.items() if path.endswith(suffix))
patterns=json.loads(get('/input.json'));exact=json.loads(get('/sdk-stdout.json'));mutant=json.loads(get('/actual-mutant-stdout.json'))
assert len(patterns)==len(exact)==len(mutant)==5618
assert get('/sdk/bisect/bisect.go')== (SDK/'src/internal/bisect/bisect.go').read_bytes()
mutated=(SDK/'src/internal/bisect/bisect.go').read_bytes().replace(b'if id&c.mask == c.bits {',b'if id&c.mask == c.bits&c.mask {')
assert get('/actual-mutant/bisect/bisect.go')==mutated
assert get('/sdk-typed-bisect')[:4]==get('/actual-mutant-typed-bisect')[:4]==b'\x7fELF'
focused=[];differences=[];original=set('q'+''.join(c) for n in range(1,5) for c in itertools.product('01yxf+-',repeat=n));original.update('q!'+p[1:] for p in list(original))
assert len(original)==5600 and original<=set(patterns)
for p,left,right in zip(patterns,exact,mutant):
 assert p==left['pattern']==right['pattern']
 m=sdk_new(p);assert left['nil_matcher']==(m is None)
 for value,wrong in zip(left['typed_ids'],right['typed_ids']):
  assert value['id']==wrong['id']
  if m is None:expected=(True,False)
  else:
   quiet,enable,conditions=m;matched=next((r for mask,bits,r in reversed(conditions) if value['id']&mask==bits),False)
   expected=(matched==enable,not quiet and matched)
  assert (value['enabled'],value['print'])==expected,(p,value)
  if (wrong['enabled'],wrong['print'])!=expected:differences.append((p,value['id']))
 a,b=sdk_result(p),native_result(p)
 assert a['allow_relative']==b['allow_relative'],(p,a,b)
 if p in ['qxyf','qxya','qxyF','q!xyf','q-xyf','qxy0','qxy','qy-xyf','q!y-xyf','qxyf+y','qxyf+y-y']:focused.append(dict(pattern=p,sdk=a,native_projection=b))
assert len(differences)==230 and any(p=='qxyf' for p,i in differences)
# Independent arbitrary-width assignments: compare tree outputs with literal
# predicate id & ((1<<width)-1)==bits, preserving earlier values/order.
def tree_value(tree,id):
 while not isinstance(tree,bool):tree,id=tree[id&1],id>>1
 return tree
rng=random.Random(772);assignments=[]
for width in range(65):
 mask=(1<<width)-1
 for bits in set([0,mask,mask>>1,1<<63] + ([1<<width] if width<64 else [])):
  for previous in [False,True]:
   tree=assign(previous,bits,width,not previous)
   ids=set([0,1,mask,mask>>1,bits,(1<<64)-1]+[rng.getrandbits(64) for _ in range(20)])
   for id in ids:assert tree_value(tree,id)==(not previous if id&mask==bits else previous),(width,bits,previous,id)
   assignments.append(dict(width=width,bits=bits,previous=previous,concrete_ids=len(ids)))
result=dict(status='READY_FOR_RUNTIME_REVIEW_SOURCE_ONLY',head='7e5c4561c82803403b579d011fb687bbe254d32a',source='445c19688aa8f669ea99e15e6517005f6156b775',archive_payloads_verified=len(raws),archived_actual_SDK_patterns=len(patterns),archived_actual_SDK_values=56180,archived_actual_mutant_differences=len(differences),all_source_projection_classes_match=True,arbitrary_width_source_projection_assignments=len(assignments),focused=focused,bindings={str(p):sha(p) for p in [SDK/'src/internal/bisect/bisect.go',SDK/'src/internal/godebug/godebug.go',ROOT/'browse/crates/symbrowse-core/src/key_sources/startup_discovery/godebug.rs',folder/'retention.json']},fresh_compiler_or_native_or_SDK_execution=False)
Path('/tmp/symaira-discovery-mask-independent-projection.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps({k:v for k,v in result.items() if k not in ['focused','bindings']}))
