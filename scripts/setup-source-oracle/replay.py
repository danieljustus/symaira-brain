#!/usr/bin/env python3
"""Actual frozen Go and native Rust source-setup processes, owned native tools."""
from __future__ import annotations
import base64
import datetime
import hashlib
import importlib.util
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
ORACLE = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
sys.path.insert(0,str(ROOT / "scripts"))
spec=importlib.util.spec_from_file_location("source_legacy",ROOT/"scripts/rust-differential.py")
legacy=importlib.util.module_from_spec(spec);sys.modules[spec.name]=legacy;spec.loader.exec_module(legacy)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def cases():
    result=[]
    def add(name,args=None,**kw):
        result.append(dict(name=name,args=args or ["setup","--from-source","<source>","--modules","browse","--json"],**kw))
    add("explicit-browse-json")
    add("explicit-browse-human",["setup","--from-source","<source>","--modules","browse"])
    for modules in ("scope,operate,browse","operate,scope","scope","operate"," browse , browse , scope ",", ,", "unknown", "browse,unknown"):
        add("selection-"+modules,modules=modules)
    for module in ("browse","operate","scope"):
        add("config-"+module,["setup","--from-source","<source>","--json"],config=f"[modules]\n{module} = true\n")
        add("env-"+module,["setup","--from-source","<source>","--json"],env={"SYMBRAIN_MODULES_"+module.upper():"true"})
    add("config-none",["setup","--from-source","<source>","--json"])
    add("project-false-nonzero-merge",["setup","--from-source","<source>","--json"],config="[modules]\nbrowse = true\n",project="[modules]\nbrowse = false\n")
    add("env-false-overrides",["setup","--from-source","<source>","--json"],config="[modules]\nbrowse = true\n",env={"SYMBRAIN_MODULES_BROWSE":"false"})
    add("missing-home",env={"HOME":"","USERPROFILE":"","HOMEDRIVE":"","HOMEPATH":""})
    add("missing-home-before-invalid-config",env={"HOME":"","USERPROFILE":"","HOMEDRIVE":"","HOMEPATH":""},config="[bad config")
    add("missing-home-before-conflict",["setup","--from-source","<source>","--fix"],env={"HOME":"","USERPROFILE":"","HOMEDRIVE":"","HOMEPATH":""})
    add("root-missing",root="missing")
    add("root-file",root="file")
    add("root-relative",relative=True)
    for args in (["setup","--modules=browse"],["setup","--from-source"],["setup","--modules"],["setup","--from-source","<source>","--fix"],["setup","--from-source","<source>","--allow-unsigned"],["setup","--help"],["setup","--help=true"],["setup","--unknown"],["setup","--from-source","<source>","--modules=browse","--force-release"],["setup","--from-source=<source>","--modules=browse","--json"],["setup","--from-source","<source>","--modules=browse","positional","--json"]):
        add("args-"+str(len(result)),args)
    for args in (["setup","----from-source","<source>","--modules","browse","--json"],
                 ["setup","--from-source","<source>","----modules","browse","--json"],
                 ["setup","--from-source","<source>","--modules","browse","----json"],
                 ["setup","--from-source","<source>","--=bad"],
                 ["setup","--from-source","<source>","--modules","browse","---json"]):
        add("flag-grammar-"+str(len(result)),args)
    for fault in ("missing","file"):add("tmp-root-"+fault,tmp_fault=fault)
    for fault in ("missing","file","valid"):add("relative-tmp-root-"+fault,relative_tmp=fault)
    if os.name!="nt":
        raw=os.fsdecode(b"bad\xff\xe2\x82")
        add("raw-module-separate",modules=raw)
        add("raw-module-inline",["setup","--from-source","<source>","--modules="+raw,"--json"])
        add("raw-module-unicode-space",modules="\u2003 "+raw+" \u00a0")
        add("raw-missing-root",["setup","--from-source","<source>/"+raw,"--modules","browse","--json"])
        add("raw-unknown-flag",["setup","--from-source","<source>","--"+raw])
        add("raw-boolean",["setup","--from-source","<source>","--json="+raw])
        for json_out in (True,False):
            args=["setup","--from-source","<source>","--modules","browse"]+(["--json"] if json_out else [])
            add("raw-existing-root-"+str(json_out),args,raw_root=os.fsdecode(b"owned\xff\xe2\x82"))
            add("raw-home-"+str(json_out),args,raw_home=os.fsdecode(b"owned\xff\xe2\x82"))
        add("literal-replacement-root",raw_root="owned\ufffd")
    if os.name!="nt":
        for fault in ("missing","file","valid","missing-payload"):
            add("raw-worker-tmp-"+fault,raw_tmp=fault)
        add("raw-home-obstructed",raw_home_fault=True)
        add("raw-root-missing-browse",raw_root=os.fsdecode(b"owned\xff\xe2\x82"),remove_browse=True)
        add("raw-root-missing-browse-human",["setup","--from-source","<source>","--modules","browse"],raw_root=os.fsdecode(b"owned\xff\xe2\x82"),remove_browse=True)
    for mode,optin in (("empty",False),("unset",False),("empty",True)):
        add("path-"+mode+"-"+str(optin),path_mode=mode,cwd_tool=True,
            env={"NoDefaultCurrentDirectoryInExePath":"",**({"GODEBUG":"execerrdot=0"} if optin else {})})
    for failure in ("git","go-version","go-build","missing-payload","directory-payload","swift-version","swift-build","swift-show"):
        add("failure-"+failure,modules="browse,operate,scope",env={"SOURCE_TOOL_FAILURE":failure})
    add("relative-tools-refused",relative_tools=True)
    add("relative-tools-explicit-optin",relative_tools=True,env={"GODEBUG":"execerrdot=0"})
    if os.name!="nt":add("go-nonexecutable-refused",nonexecutable=True)
    if os.name=="nt":
        add("implicit-cwd-tool-optin-distinct-path",cwd_tool=True,env={"GODEBUG":"execerrdot=0","SOURCE_DISTINCT_CWD":"yes"},expected_commit="c650123456789abcdef0123456789abcdef0123456")
        add("implicit-cwd-tool-optin-only",cwd_tool=True,omit="git",env={"GODEBUG":"execerrdot=0","SOURCE_DISTINCT_CWD":"yes"},expected_commit="c650123456789abcdef0123456789abcdef0123456")
        add("implicit-cwd-tool-optin-disabled",cwd_tool=True,env={"GODEBUG":"execerrdot=0","SOURCE_DISTINCT_CWD":"yes","NoDefaultCurrentDirectoryInExePath":""})
        add("implicit-cwd-tool-refused",cwd_tool=True)
        add("implicit-cwd-tool-disabled",cwd_tool=True,env={"NoDefaultCurrentDirectoryInExePath":""})
        add("implicit-cwd-same-file",cwd_hardlink=True)
    if os.name=="nt":
        for policy in ("same","different"):
            add("explicit-relative-later-absolute-"+policy,relative_then_absolute=policy,
                env={"NoDefaultCurrentDirectoryInExePath":""})
    add("missing-git",omit="git")
    add("missing-go",omit="go")
    add("missing-swift",omit="swift",modules="browse,operate,scope")
    add("missing-browse-dir",remove_browse=True)
    add("probe-exit",env={"SOURCE_PROBE_EXIT":"yes"})
    add("probe-timeout",env={"SOURCE_PROBE_DELAY":"yes"})
    for index,payload in enumerate((b"{}",b"null",b"{bad",b'{"version":1}',b'{"VERSION":"native-case"}',b'{"version":"native-case","version":null}',b'{"version":"0.1","unknown":1e1000}')):
        add("probe-json-"+str(index),env={"SOURCE_PROBE_BYTES":base64.b64encode(payload).decode()})
    for fault in ("managed-file", "binary-directory", "sidecar-directory"):
        add("publication-"+fault,publication=fault)
    for index, identity in enumerate((b"tool <identity> & UTF8 \xe2\x80\xa8\n", b"invalid \xff\xe2\x82\n", b"", b"tool\nsecond line\n", b"\xc2\xa0valid tool\xc2\xa0\n", b"\xc2\xa0invalid\xff\xe2\x82\xc2\xa0\n", b" invalid\xff\xe2\x82 \n")):
        add("tool-identity-"+str(index),env={"SOURCE_TOOL_VERSION_BYTES":base64.b64encode(identity).decode() or "="})
    add("real-go-build",real_go=True)
    add("preserve-other-owner",foreign=True)
    for lexical in ("dot","parent","slash"):
        add("managed-home-"+lexical,home_lexical=lexical)
    if os.name!="nt":add("managed-home-symlink-parent",home_lexical="symlink-parent")
    return result


def normalize(data,root):
    data=legacy.normalize_fixture_root(data,root)
    # Both real implementations create and remove a unique staging directory.
    # Only that observed owned prefix is replaced; all tool argv and paths remain.
    data=re.sub(rb"symbrain-source-build-[A-Za-z0-9]+",b"symbrain-source-build-<unique>",data)
    return re.sub(rb"(?<=[/\\])(\.install-|\.provenance-)[A-Za-z0-9]+",rb"\1<unique>",data)


def filesystem(root,originals,started,ended,expected_commit="7650123456789abcdef0123456789abcdef0123456"):
    result={}
    for path in sorted(root.rglob("*")):
        rel=path.relative_to(root).as_posix(); mode=stat.S_IMODE(path.lstat().st_mode)
        if path.is_symlink():result[rel]={"type":"link","mode":mode,"target":str(path.readlink())};continue
        if path.is_dir():result[rel]={"type":"dir","mode":mode};continue
        content=normalize(path.read_bytes(),root)
        if rel.endswith(".provenance.json") and content!=originals.get(rel):
            record=json.loads(content);assert record["source"]=="brain-source" and record["version"]==""
            assert record["receiver_commit"]==expected_commit
            timestamp=record["built_at"];when=datetime.datetime.fromisoformat(timestamp.replace("Z","+00:00")).timestamp()
            assert timestamp.endswith("Z") and started-1<=when<=ended+1
            payload=path.with_name(path.name.removesuffix(".provenance.json"))
            assert digest(payload)==record["binary_sha256"],"provenance does not identify installed bytes"
            content=content.replace(timestamp.encode(),b"<validated-install-timestamp>")
        result[rel]={"type":"file","mode":mode,"size":len(content),"sha256":hashlib.sha256(content).hexdigest()}
        if len(content)<8192:result[rel]["content_base64"]=base64.b64encode(content).decode()
    assert not any(name.startswith("tmp/symbrain-source-build-") for name in result),"staging residue"
    return result


def configure(case,root,go,tool):
    env=legacy.prepare_root(root,go);env.update({"CI":"true","SYMBRAIN_GO_BINARY":str(root/"absent-fallback"),"SOURCE_TOOL_LOG":str(root/"invocations.jsonl")})
    tools=root/"tools";tools.mkdir(); source=root/"receiving sources";source.mkdir();(source/"browse").mkdir()
    for module in ("operate","scope"):(source/module).mkdir()
    if case.get("raw_root"):
        source=source/case["raw_root"];source.mkdir()
        for module in ("browse","operate","scope"):(source/module).mkdir()
    if case.get("raw_home"):
        home=root/case["raw_home"];home.mkdir();env["HOME"]=str(home)
    for name in ("git","go","swift"):
        if case.get("omit")==name:continue
        dest=tools/(name);shutil.copyfile(tool,dest);dest.chmod(0o755)
    env["PATH"]=os.path.relpath(tools,env["PROJECT"]) if case.get("relative_tools") else str(tools)
    if case.get("nonexecutable"):(tools/"go").chmod(0o644)
    if case.get("cwd_tool"):shutil.copyfile(tool,Path(env["PROJECT"])/"git")
    if case.get("cwd_hardlink"):os.link(tools/"git",Path(env["PROJECT"])/"git")
    if case.get("relative_then_absolute"):
        later=tools
        if case["relative_then_absolute"]=="different":
            later=root/"other-tools";later.mkdir()
            for name in ("git","go","swift"):shutil.copyfile(tool,later/name)
        env["PATH"]=os.path.relpath(tools,env["PROJECT"])+os.pathsep+str(later)
    if case.get("path_mode"):
        if case["path_mode"]=="unset":env.pop("PATH",None)
        else:env["PATH"]=""
    # Go and Rust agree that an all-empty PATHEXT explicitly permits raw PE names.
    if os.name=="nt":env["PATHEXT"]=";;"
    tmp=root/"tmp";tmp.mkdir()
    for key in ("TMPDIR","TMP","TEMP"):env[key]=str(tmp)
    if case.get("tmp_fault"):
        bad=root/"tmp-obstruction"
        if case["tmp_fault"]=="file":bad.write_text("owned tempfile obstruction")
        for key in ("TMPDIR","TMP","TEMP"):env[key]=str(bad)
    if case.get("relative_tmp"):
        bad=Path(env["PROJECT"])/"worker-tmp"
        if case["relative_tmp"]=="file":bad.write_text("owned relative obstruction")
        if case["relative_tmp"]=="valid":bad.mkdir()
        for key in ("TMPDIR","TMP","TEMP"):env[key]="worker-tmp"
    if case.get("raw_tmp"):
        bad=root/os.fsdecode(b"worker\xff\xe2\x82")
        if case["raw_tmp"]=="file":bad.write_bytes(b"owned obstruction")
        if case["raw_tmp"] in ("valid","missing-payload"):bad.mkdir()
        for key in ("TMPDIR","TMP","TEMP"):env[key]=str(bad)
        if case["raw_tmp"]=="missing-payload":env["SOURCE_TOOL_FAILURE"]="missing-payload"
    if case.get("raw_home_fault"):
        home=root/os.fsdecode(b"home\xff\xe2\x82");(home/".symaira").mkdir(parents=True)
        (home/".symaira/bin").write_bytes(b"owned obstruction");env["HOME"]=str(home)
    if case.get("config"):
        cfg=Path(env["XDG_CONFIG_HOME"])/"symbrain/config.toml";cfg.parent.mkdir();cfg.write_text(case["config"])
    if case.get("project"):(Path(env["PROJECT"])/".symbrain.toml").write_text(case["project"])
    env.update(case.get("env",{}))
    if case.get("home_lexical"):
        lexical=case["home_lexical"];home=env["HOME"]
        if lexical=="dot":home+="/./"
        elif lexical=="parent":home+="/../"+Path(home).name
        elif lexical=="slash":home+="//"
        elif lexical=="symlink-parent":
            (root/"owner/nested").mkdir(parents=True)
            (root/"link").symlink_to("owner/nested",target_is_directory=True)
            home=str(root/"link")+"/../home"
        env["HOME"]=home
        if os.name=="nt":env["USERPROFILE"]=home
    if case.get("remove_browse"):shutil.rmtree(source/"browse")
    if case.get("root")=="missing":source=source/"missing"
    if case.get("root")=="file":source=source/"file";source.write_text("owned root file")
    selected=os.path.relpath(source,env["PROJECT"]) if case.get("relative") else str(source)
    args=[arg.replace("<source>",selected) for arg in case["args"]]
    if "modules" in case:
        args[args.index("--modules")+1]=case["modules"]
    if case.get("foreign"):
        foreign=root/"home/.local/bin";foreign.mkdir(parents=True);(foreign/"symbrowse").write_text("other owner's binary")
        managed=root/"home/.symaira/bin";managed.mkdir(parents=True);(managed/"unrelated").write_text("unrelated managed payload")
    fault=case.get("publication")
    if fault:
        managed=root/"home/.symaira/bin";managed.parent.mkdir(parents=True)
        if fault=="managed-file":managed.write_text("preserve managed-directory obstruction")
        else:
            managed.mkdir()
            if fault=="binary-directory":(managed/"symbrowse").mkdir()
            else:(managed/"symbrowse.provenance.json").mkdir()
    if case.get("real_go"):
        tool_path=Path(shutil.which("go"))
        shutil.copyfile(tool_path,tools/"go");(tools/"go").chmod(0o755)
        env.update(GOROOT=subprocess.check_output([str(tool_path),"env","GOROOT"],text=True).strip(),
                   GOCACHE=os.environ["SOURCE_REAL_GO_CACHE"],GOTOOLCHAIN="local",GO111MODULE="on",GOTELEMETRY="off")
        telemetry=root/"config/go/telemetry";telemetry.mkdir(parents=True)
        (telemetry/"mode").write_text("off\n")
        env["GOTELEMETRYDIR"]=str(telemetry)
        (source/"browse/go.mod").write_text("module fixture.example/nativeworker\n\ngo 1.26\n")
        cmd=source/"browse/cmd/symbrowse";cmd.mkdir(parents=True)
        (cmd/"main.go").write_text('package main\nimport "fmt"\nfunc main(){fmt.Print(`{"version":"0.765.0-real"}`)}\n')
    return env,args


def observe(binary,case,root,go,tool,control=None):
    env,args=configure(case,root,go,tool)
    if control:env.update(SOURCE_CONTROL_TARGET=str(control[0]),SOURCE_CONTROL_MUTATION=control[1])
    originals={p.relative_to(root).as_posix():normalize(p.read_bytes(),root) for p in root.rglob("*.provenance.json") if p.is_file()}
    commit=case.get("expected_commit","7650123456789abcdef0123456789abcdef0123456")
    started=time.time();before=filesystem(root,originals,started,started,commit)
    proc=subprocess.run([str(binary),*args],cwd=env["PROJECT"],env=env,capture_output=True,timeout=25);ended=time.time()
    contract={"exit":proc.returncode,"stdout_base64":base64.b64encode(normalize(proc.stdout,root)).decode(),"stderr_base64":base64.b64encode(normalize(proc.stderr,root)).decode(),"filesystem":filesystem(root,originals,started,ended,commit)}
    return {"started":started,"ended":ended,"actual_exit":proc.returncode,"raw_stdout_base64":base64.b64encode(proc.stdout).decode(),"raw_stderr_base64":base64.b64encode(proc.stderr).decode(),"before":before,"contract":contract}


def native_cleanup(rust,go,tool):
    with tempfile.TemporaryDirectory(prefix="source-cleanup-owned-") as fixture:
        root=Path(fixture);case=cases()[0];env,args=configure(case,root,go,tool)
        pid_file=root/"owned-descendant.pid";env["SOURCE_DESCENDANT_PID"]=str(pid_file)
        started=time.monotonic()
        result=subprocess.run([str(rust),*args],cwd=env["PROJECT"],env=env,capture_output=True,timeout=20)
        assert result.returncode==0,(result.stdout,result.stderr)
        pid=int(pid_file.read_text())
        def active():
            if os.name=="nt":
                output=subprocess.check_output([str(Path(env["SystemRoot"])/"System32/tasklist.exe"),"/FI",f"PID eq {pid}","/FO","CSV","/NH"])
                return f'"{pid}"'.encode() in output
            status=Path(f"/proc/{pid}/stat")
            if status.exists() and status.read_text().split(") ",1)[1].startswith("Z "):return False
            return subprocess.run(["/bin/kill","-0",str(pid)],capture_output=True).returncode==0
        deadline=time.monotonic()+2
        while active() and time.monotonic()<deadline:time.sleep(0.02)
        assert not active(),"owned build descendant survived successful source setup"
        return {"actual_native_process":True,"exit":result.returncode,"descendant_pid":pid,"descendant_active_after_return":False,"elapsed_seconds":time.monotonic()-started,"raw_stdout_base64":base64.b64encode(result.stdout).decode(),"raw_stderr_base64":base64.b64encode(result.stderr).decode(),"scope":"Successful builder cleanup, with owned process-group/Job descendant; no hard-kill cancellation claim."}


def main():
    go,rust,report=(Path(p).resolve() for p in sys.argv[1:4]);control=sys.argv[4] if len(sys.argv)>4 else None
    observations=[];failures=[]; selected=cases() if not control else cases()[:1]
    cleanup=None
    with tempfile.TemporaryDirectory(prefix="setup-source-tools-") as working:
        working=Path(working);src=working/"fixture.go";src.write_bytes((ROOT/"scripts/setup-source-oracle/tool_fixture.go.txt").read_bytes());tool=working/("tool.exe" if os.name=="nt" else "tool")
        subprocess.run(["go","build","-trimpath","-o",str(tool),str(src)],cwd=working,env={**os.environ,"CGO_ENABLED":"0","GO111MODULE":"off"},check=True)
        tool_digest=digest(tool)
        candidate=rust
        if control:
            candidate=working/("control.exe" if os.name=="nt" else "control");shutil.copyfile(tool,candidate);candidate.chmod(0o755)
        real_cache=working/"real-go-cache";real_cache.mkdir();os.environ["SOURCE_REAL_GO_CACHE"]=str(real_cache)
        for case in selected:
            item={"case":case["name"]}
            for label,binary in (("go",go),("rust",candidate)):
                with tempfile.TemporaryDirectory(prefix="setup-source-case-") as fixture:
                    item[label]=observe(binary,case,Path(fixture),go,tool,(rust,control) if label=="rust" and control else None)
            fields=[key for key in item["go"]["contract"] if item["go"]["contract"][key]!=item["rust"]["contract"][key]]
            if fields:failures.append({"case":case["name"],"fields":fields});print("FAIL",case["name"],fields)
            observations.append(item)
        if not control:cleanup=native_cleanup(rust,go,tool)
    names=subprocess.check_output(["git","diff","--name-only",ORACLE,"HEAD"],cwd=ROOT,text=True).splitlines()
    names+=subprocess.check_output(["git","diff","--name-only"],cwd=ROOT,text=True).splitlines()
    names+=subprocess.check_output(["git","ls-files","--others","--exclude-standard"],cwd=ROOT,text=True).splitlines()
    data={"go_oracle_ref":ORACLE,"candidate_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),"candidate_dirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=ROOT)),"runtime":platform.platform(),"go_sdk":subprocess.check_output(["go","version"],text=True).strip(),"go_binary_sha256":digest(go),"rust_binary_sha256":digest(rust),"tool_binary_sha256":tool_digest,"real_worker_toolchain":"Actual go build -trimpath ./cmd/symbrowse on owned dependency-free module; shared owned cache outside individual fixture snapshots, removed with the runner temporary directory.","total":len(selected),"matched":len(selected)-len(failures),"complete_observations":len(observations),"control":control,"native_cleanup":cleanup,"exit":int(bool(failures)),"failures":failures,"observations":observations,"comparison":"Exact stdout, stderr, exit and full owned fixture files/types/modes/hashes, including real tool argv/cwd/CGO and actual installed-payload hash. Only owned fixture roots, observed unique source staging paths, and new validated UTC install timestamps normalize. No failures or files are dropped.","remaining_scope":"Full typed invalid-configuration diagnostics remain Go-owned; hardware, signing and native macOS/Windows execution are not established by this Linux receipt.","candidate_source_sha256":{name:digest(ROOT/name) for name in sorted(set(names)) if (ROOT/name).is_file() and not name.startswith("migration/evidence/")},"go_source_sha256":{name:hashlib.sha256(subprocess.check_output(["git","show",f"{ORACLE}:{name}"],cwd=ROOT)).hexdigest() for name in ("cmd/symbrain/cmd_setup.go","cmd/symbrain/cmd_setup_source.go","internal/managed/install.go","internal/managed/provenance.go")}}
    report.write_text(json.dumps(data,indent=2)+"\n");print(f"Source setup: {data['matched']}/{data['total']} matched");return data["exit"]


if __name__=="__main__":raise SystemExit(main())
