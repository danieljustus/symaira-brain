"""Execute extracted actual Windows SDK pure Clean against actual Rust body.

Linux-only algorithm evidence; no Windows process/filesystem/runtime claim.
"""
import pathlib,hashlib,json,subprocess,tempfile,itertools
ROOT=pathlib.Path('/workspace/symaira-doctor765-windows')
SDK=pathlib.Path('/workspace/toolchains/go1.26.7/src')
digest=lambda b:hashlib.sha256(b).hexdigest()
base=(SDK/'internal/filepathlite/path.go').read_text()
win=(SDK/'internal/filepathlite/path_windows.go').read_text()
def block(text,name):
    start=text.index(name);end=text.index('\n}',start)+2;return text[start:end]
functions=[block(base,'type lazybuf struct {')]
for name in ['func (b *lazybuf) index(','func (b *lazybuf) append(','func (b *lazybuf) prepend(','func (b *lazybuf) string(','func Clean(']:functions.append(block(base,name))
for name in ['func FromSlash(','func replaceStringByte(']:functions.append(block(base,name).replace('stringslite.IndexByte','strings.IndexByte'))
for name in ['func IsPathSeparator(','func volumeNameLen(','func pathHasPrefixFold(','func uncLen(','func cutPath(','func postClean(','func toUpper(']:functions.append(block(win,name))
join_source=(SDK/'path/filepath/path_windows.go').read_text()
functions.append(block(join_source,'func join(').replace('os.IsPathSeparator','IsPathSeparator'))
go='package main\nimport("bufio";"encoding/hex";"fmt";"os";"strings";"slices")\nconst Separator=byte(92)\n'+ '\n'.join(functions)+'\nfunc main(){s:=bufio.NewScanner(os.Stdin);for s.Scan(){b,e:=hex.DecodeString(s.Text());if e!=nil{panic(e)};fmt.Printf("%x\\n",[]byte(join([]string{string(b),".symaira","bin"})))};if e:=s.Err();e!=nil{panic(e)}}\n'
actual=(ROOT/'rust/symbrain-cli/src/managed_home.rs').read_text()
join_body=actual[actual.index('    let mut joined ='):actual.index('    #[cfg(unix)]')].replace('cfg!(windows)','true')
rust='''#![allow(dead_code)]
mod symbrain_core { pub mod config {
 pub fn os_bytes(s: &std::ffi::OsStr) -> std::borrow::Cow<'_,[u8]> {
  use std::os::unix::ffi::OsStrExt; std::borrow::Cow::Borrowed(s.as_bytes())
 }
}}
mod reviewed { use crate::symbrain_core;
'''+actual.replace('//! The managed owner boundary follows Go filepath.Join/Clean, never symlinks.','// Exact actual reviewed body; only surrounding environment byte helper stubbed.')+'''
pub fn check(input: &[u8]) -> Vec<u8> { let home=std::ffi::OsStr::new(std::str::from_utf8(input).unwrap());
'''+join_body+'''
bytes }
}
fn main() {use std::io::{BufRead,Write};let stdin=std::io::stdin();let mut out=std::io::BufWriter::new(std::io::stdout());for line in stdin.lock().lines(){let line=line.unwrap();let bytes=(0..line.len()).step_by(2).map(|i|u8::from_str_radix(&line[i..i+2],16).unwrap()).collect::<Vec<_>>();for b in reviewed::check(&bytes){write!(out,"{b:02x}").unwrap();}writeln!(out).unwrap();}}
'''
roots=['','a','..','.','C:','C:\\','C:/','c:relative','1:','\\','\\\\','\\\\host','\\\\host\\share','\\\\.','\\\\?','\\??','\\\\?\\C:','\\\\?\\C:\\','\\\\.\\UNC\\host\\share','//host/share','漢字','a:b']
parts=['a','.','..','','x:y','??','c:','home','ü','漢字']
inputs=set()
for root,separator,length in itertools.product(roots,['/','\\','//','\\\\'],range(4)):
    for path in itertools.product(parts,repeat=length):
        value=root+(separator if root and path else '')+separator.join(path)
        inputs.add(value.encode());inputs.add((value+separator+'.symaira'+separator+'bin').encode())
values=sorted(inputs);data=b''.join(x.hex().encode()+b'\n' for x in values)
with tempfile.TemporaryDirectory(prefix='third-windows-lexical-') as temporary:
    tmp=pathlib.Path(temporary);gp=tmp/'sdk.go';rp=tmp/'actual.rs';gp.write_text(go);rp.write_text(rust)
    subprocess.run(['/workspace/toolchains/go1.26.7/bin/go','build','-trimpath','-o',str(tmp/'sdk'),str(gp)],check=True)
    subprocess.run(['rustc','--edition=2024','-C','debuginfo=0','-o',str(tmp/'actual'),str(rp)],check=True)
    left=subprocess.run([str(tmp/'sdk')],input=data,capture_output=True,check=True).stdout.splitlines()
    right=subprocess.run([str(tmp/'actual')],input=data,capture_output=True,check=True).stdout.splitlines()
    assert len(left)==len(right)==len(values)
    differences=[dict(input_hex=values[i].hex(),sdk_hex=a.decode(),rust_hex=b.decode()) for i,(a,b) in enumerate(zip(left,right)) if a!=b]
    report=dict(head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),actual_source_sha256=digest(actual.encode()),cases=len(values),differences=differences,
                inputs_sha256=digest(data),sdk_outputs_sha256=digest(b'\n'.join(left)),rust_outputs_sha256=digest(b'\n'.join(right)),go_probe_source=go,rust_probe_source=rust,
                sdk_input_sha256={'path.go':digest(base.encode()),'path_windows.go':digest(win.encode()),'path/filepath/path_windows.go':digest(join_source.encode())},go_probe_sha256=digest((tmp/'sdk').read_bytes()),rust_probe_sha256=digest((tmp/'actual').read_bytes()),
                scope='Actual extracted Windows Go1.26.7 Join/Clean/FromSlash versus actual managed_path assembly/clean body with only cfg!(windows) set true and byte-to-OsStr interface for valid UTF8 HOME. Surrounding os_bytes helper provides identical valid UTF8 bytes on Linux. Internal SDK IsPathSeparator/IndexByte bridged to identical standalone functions. Not native Windows runtime or filesystem evidence.')
    pathlib.Path('/tmp/symaira-doctor-source-independent-third-windows-managed-join.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ['cases','differences','scope']}))
    assert not differences
