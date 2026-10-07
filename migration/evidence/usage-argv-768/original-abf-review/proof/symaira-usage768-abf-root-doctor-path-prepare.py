from pathlib import Path
import hashlib,json
sdk=Path('/workspace/toolchains/go1.26.7/src')
repo=Path('/workspace/symaira-usage768-local-files')
out=Path('/tmp/symaira-usage768-abf-root-doctor-path')
out.mkdir(exist_ok=True)
production=repo/'rust/symbrain-usage/src/provider_config/credential_path.rs'
s=production.read_text()
join=s[:s.index('#[cfg(unix)]')]
join=join.replace('base: &Path, child: &str) -> PathBuf','base: &[u16], child: &[u16]) -> Vec<u16>').replace('credential_path_units(base)','base.to_vec()').replace('credential_path_units(Path::new(child))','child.to_vec()').replace('PathBuf::new()','Vec::new()')
pure=s[s.index('fn credential_path_sep'):]
pure=pure.replace('#[cfg(not(windows))]\nfn credential_volume_len(_: &[u16]) -> usize {\n    0\n}\n','').replace('#[cfg(windows)]\n','')
rust=(join+pure).replace('cfg!(windows)','true')
rust+='''\nfn credential_path_from_units(p: &[u16])->Vec<u16>{p.to_vec()}
fn parse(s:&str)->Vec<u16>{if s.is_empty(){return Vec::new()}s.split(',').map(|s|u16::from_str_radix(s,16).unwrap()).collect()}
fn encode(p:&[u16])->String{p.iter().map(|u|format!("{u:04x}")).collect::<Vec<_>>().join(",")}
fn main(){use std::io::BufRead;for l in std::io::stdin().lock().lines(){let l=l.unwrap();let(a,b)=l.split_once('|').unwrap();let a=parse(a);let b=parse(b);println!("{}|{}",encode(&credential_clean_units(&a)),encode(&credential_join(&a,&b)));}}
'''
(out/'path.rs').write_text(rust)
win=(sdk/'internal/filepathlite/path_windows.go').read_text()
common=(sdk/'internal/filepathlite/path.go').read_text()
join=(sdk/'path/filepath/path_windows.go').read_text()
wtf=(sdk/'syscall/wtf8_windows.go').read_text()
go='package main\nimport("bufio";"os";"fmt";"slices";"strings";stringslite "strings";"unicode/utf16";"unicode/utf8";"strconv")\nconst Separator=byte(92)\nfunc IsPathSeparator(c uint8)bool{return c==92||c==47}\n'
go+=common[common.index('type lazybuf'):common.index('// IsLocal is')]
go+=common[common.index('func FromSlash'):common.index('// Split is')]
go+=win[win.index('func toUpper'):win.index('// IsAbs reports')]
go+=win[win.index('func volumeNameLen'):]
go+=join[join.index('func join('):join.index('func sameWord')].replace('os.IsPathSeparator','IsPathSeparator')
go+=wtf[wtf.index('const ('):]
go+='''
func parse(s string)[]uint16{out:=[]uint16{};if s==""{return out};for _,v:=range strings.Split(s,","){u,e:=strconv.ParseUint(v,16,16);if e!=nil{panic(e)};out=append(out,uint16(u))};return out}
func encode(s string)string{out:=[]string{};for _,u:=range encodeWTF16(s,nil){out=append(out,fmt.Sprintf("%04x",u))};return strings.Join(out,",")}
func main(){scan:=bufio.NewScanner(os.Stdin);for scan.Scan(){a,b,ok:=strings.Cut(scan.Text(),"|");if !ok{panic("input")};base:=string(decodeWTF16(parse(a),nil));child:=string(decodeWTF16(parse(b),nil));fmt.Printf("%s|%s\\n",encode(Clean(base)),encode(join([]string{base,child})))};if scan.Err()!=nil{panic(scan.Err())}}
'''
(out/'path.go').write_text(go)
receipt=dict(production_sha256=hashlib.sha256(production.read_bytes()).hexdigest(),sdk_sha256={str(p):hashlib.sha256(p.read_bytes()).hexdigest() for p in [sdk/'internal/filepathlite/path_windows.go',sdk/'internal/filepathlite/path.go',sdk/'path/filepath/path_windows.go',sdk/'syscall/wtf8_windows.go']},transforms=['Rust original Join signature/OS-unit adapters mechanically accept Vec<u16>; cfg Windows pure branches selected; exact Clean/volume/prefix/UNC bodies unchanged','Go SDK original lazybuf/Clean/FromSlash/replaceStringByte/volume/prefix/UNC/postClean/Join/WTF16 encode/decode copied; imports made host-accessible; Join os.IsPathSeparator changed to copied Windows separator; no filesystem calls'],scope='Linux execution of copied actual Windows SDK pure algorithms and production pure unit helpers; not Windows runtime, filesystem, environment or ACL proof')
(out/'source.json').write_text(json.dumps(receipt,indent=2)+'\n')
