#!/usr/bin/env python3
"""Source projections only: no actual Go/Rust/provider execution or target."""
from pathlib import Path
import hashlib,itertools,json
ROOT=Path(__file__).resolve().parents[4]
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

