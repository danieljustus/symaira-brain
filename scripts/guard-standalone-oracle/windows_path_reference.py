#!/usr/bin/env python3
"""Actual copied Go SDK Windows algorithms vs production Rust helper; NOT native Windows proof."""
import gzip
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import replay

FILES=['src/internal/filepathlite/path.go','src/internal/filepathlite/path_windows.go',
       'src/path/filepath/path_windows.go','src/path/filepath/path_test.go','LICENSE',
       'src/syscall/env_windows.go','src/syscall/syscall_windows.go']


def section(text,start):
    begin=text.index(start);end=text.index('\n}',begin)+2
    return text[begin:end]


def main():
    report=Path(sys.argv[1]);root=replay.ROOT
    sdk=Path(subprocess.check_output(['go','env','GOROOT'],text=True).strip())
    version=subprocess.check_output(['go','version'],text=True).strip()
    assert 'go1.26.7 ' in version,version
    manifest=json.loads((root/'scripts/guard-standalone-oracle/windows-sdk-source.json').read_text())
    source={name:(sdk/name).read_bytes() for name in FILES}
    for name,raw in source.items():assert replay.digest(raw)==manifest[name], 'SDK reference bytes changed: '+name
    lite=source[FILES[0]].decode();win=source[FILES[1]].decode();join=source[FILES[2]].decode()
    copied=lite[lite.index('type lazybuf struct'):lite.index('// IsLocal')]
    copied+='\n'+section(lite,'func FromSlash(')+'\n'+section(lite,'func replaceStringByte(').replace('stringslite.IndexByte','strings.IndexByte')
    for name in ['IsPathSeparator','toUpper','volumeNameLen','pathHasPrefixFold','uncLen','cutPath','postClean']:
        copied+='\n'+section(win,'func '+name+'(')
    original_join=section(join,'func join(')
    adapted_join=original_join.replace('os.IsPathSeparator','IsPathSeparator')
    assert adapted_join!=original_join
    driver='''\nfunc main() {
 var rows []struct { Mode string; BaseHex string; Parts []string }
 if err:=json.NewDecoder(os.Stdin).Decode(&rows);err!=nil { panic(err) }
 results:=[]string{}
 for _,row:=range rows { base,err:=hex.DecodeString(row.BaseHex);if err!=nil {panic(err)}
 value:="";if row.Mode=="clean" {value=Clean(string(base))}else{value=join(append([]string{string(base)},row.Parts...))}
 results=append(results,hex.EncodeToString([]byte(value))) }
 if err:=json.NewEncoder(os.Stdout).Encode(results);err!=nil { panic(err) }
}
'''
    actual='// Copyright 2024 The Go Authors. BSD-3-Clause; SDK LICENSE retained.\npackage main\nimport("slices";"strings";"encoding/json";"encoding/hex";"os")\n'
    actual+="const Separator='\\\\'\n"
    actual+=copied+'\n'+adapted_join+driver
    bases=['','.', '..','../..','a/..','a/../c:','a/../c:/a','a/../../c:',
           'foo:bar','foo:bar/','foo:bar/a/..','foo:bar\\a\\..\\','foo:bar/a/../b','foo:bar\\a\\..\\b','foo:','c:','c:.','c:foo','c:foo/../../bar','c:/',r'C:\a\..',
           '/',r'\a\..\??',r'\??\C:\a\..',r'\\host\share',r'\\host\share\foo\..\..',
           r'\\?\C:\a\..',r'\\?\UNC\host\share\a\..',r'\\.\UNC\host\share\..',
           r'\\.\C:\a\..',r'\\i\..\c$',r'\\',r'\\abc\\','///abc','雪/../cfg','a\ufffd/../cfg','foo:/a/../../雪a','foo:/a/../../éa','雪a:/a/../../abcd','a/../\ud800','\ud800:','雪:','\udc00:/a/..','C:/\ud800/a/../b']
    # Full pinned Go own Windows Clean examples, including their literal raw paths.
    import re
    own=source[FILES[3]].decode().split('var wincleantests = []PathTest{',1)[1].split('\n}',1)[0]
    sdk_cases=re.findall(r'\{`([^`]*)`, `([^`]*)`\}',own)
    rows=[dict(mode='clean',base=base,parts=[]) for base in bases+[case[0] for case in sdk_cases]]
    for base in bases:
        for parts in [[],['symguard','config.toml'],['.config','symguard','config.toml'],['','b'],['??','C:','a'],['/a'],['..','b']]:
            rows.append(dict(mode='join',base=base,parts=parts))
    with tempfile.TemporaryDirectory(prefix='guard770-win-source-reference-') as directory:
        owned=Path(directory);go_source=owned/'main.go';go_source.write_text(actual)
        go_binary=owned/'go-reference';rust_binary=owned/'native-reference'
        if os.name=='nt':go_binary=go_binary.with_suffix('.exe');rust_binary=rust_binary.with_suffix('.exe')
        subprocess.run(['go','build','-o',str(go_binary),str(go_source)],check=True,env=dict(os.environ,CGO_ENABLED='0',GOTOOLCHAIN='go1.26.7'))
        production=root/'rust/symguard-cli/src/guard_path_windows.rs'
        rust_source=owned/'main.rs'
        rust_source.write_text('''use std::io::{self,Read};
#[path = PRODUCTION] mod windows;
fn bytes(value:&str)->Vec<u8> {value.as_bytes().chunks_exact(2).map(|c|u8::from_str_radix(std::str::from_utf8(c).unwrap(),16).unwrap()).collect()}
fn main() { let mut input=String::new();io::stdin().read_to_string(&mut input).unwrap();
for line in input.lines() { let mut fields=line.split('\\t');let mode=fields.next().unwrap();
let base:Vec<u16>=bytes(fields.next().unwrap()).chunks_exact(2).map(|c|u16::from_le_bytes([c[0],c[1]])).collect();
let parts:Vec<String>=fields.map(|v|String::from_utf8(bytes(v)).unwrap()).collect();let refs:Vec<&str>=parts.iter().map(String::as_str).collect();
let out=if mode=="clean" {windows::clean_units(&base)}else{windows::join_units(&base,&refs)};println!("{}",out.iter().flat_map(|u|u.to_le_bytes()).map(|b|format!("{b:02x}")).collect::<String>()); } }
'''.replace('PRODUCTION',json.dumps(str(production))))
        subprocess.run(['rustc','--edition=2024','-C','debuginfo=0','-D','warnings',str(rust_source),'-o',str(rust_binary)],check=True)
        go_rows=[dict(Mode=row['mode'],BaseHex=row['base'].encode('utf-8','surrogatepass').hex(),Parts=row['parts']) for row in rows]
        expected_hex=json.loads(subprocess.check_output([str(go_binary)],input=json.dumps(go_rows).encode()))
        expected=[bytes.fromhex(raw).decode('utf-8','surrogatepass') for raw in expected_hex]
        wire='\n'.join('\t'.join([row['mode'],row['base'].encode('utf-16le','surrogatepass').hex()]+[part.encode().hex() for part in row['parts']]) for row in rows)+'\n'
        observed=[bytes.fromhex(line).decode('utf-16le','surrogatepass') for line in subprocess.check_output([str(rust_binary)],input=wire.encode()).decode().splitlines()]
        assert len(expected)==len(observed)==len(rows)
        results=[dict(**row,go=go,native=native,matched=go==native) for row,go,native in zip(rows,expected,observed)]
        for input,output in sdk_cases:
            assert expected[next(i for i,row in enumerate(rows) if row['mode']=='clean' and row['base']==input)]==output
        evidence=report.parent/(report.stem+'-artifacts');evidence.mkdir()
        records=[]
        for path in [go_source,rust_source,go_binary,rust_binary]:
            raw=path.read_bytes();dest=evidence/(path.name+'.gz');dest.write_bytes(gzip.compress(raw,mtime=0));assert gzip.decompress(dest.read_bytes())==raw
            records.append(dict(original=str(path),retained=str(dest),sha256=replay.digest(raw),bytes=len(raw)))
    output=dict(candidate_head=subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),
                candidate_dirty=bool(subprocess.check_output(['git','status','--porcelain'],cwd=root)),
                sdk_version=version,sdk_source_sha256=manifest,production_helper_sha256=replay.digest(production.read_bytes()),
                copied_reference_transform=['extract complete SDK Clean/lazybuf and Windows volume/postClean functions',
                    'extract SDK FromSlash/replaceStringByte','replace join os.IsPathSeparator with copied Windows IsPathSeparator',
                    'replace internal stringslite.IndexByte with identical public strings.IndexByte',
                    'add isolated JSON/hex process driver preserving Go WTF8 and native UTF16 units; no frozen Go source modifications'],
                native_windows_runtime_proof=False,sdk_own_clean_cases=len(sdk_cases),total=len(rows),matched=sum(row['matched'] for row in results),results=results,artifacts=records)
    report.write_text(json.dumps(output,indent=2)+'\n')
    assert all(row['matched'] for row in results), 'see exact source-reference mismatch receipt'
    print(f'Windows SDK source reference {len(rows)}/{len(rows)}; {len(sdk_cases)} SDK Clean examples; NOT native Windows proof')


if __name__=='__main__':main()
