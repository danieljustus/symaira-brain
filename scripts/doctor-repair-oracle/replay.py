#!/usr/bin/env python3
"""Real Doctor repair processes, strict contracts, immutable Go CLI oracle."""
from __future__ import annotations
import base64
import calendar
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
from cases import cases

ROOT = Path(__file__).resolve().parents[2]
ORACLE = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("doctor_legacy", ROOT / "scripts/rust-differential.py")
legacy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = legacy
spec.loader.exec_module(legacy)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def build_probe(root):
    source = root / "main.go"
    source.write_bytes((ROOT / "scripts/doctor-repair-oracle/probe_fixture.go.txt").read_bytes())
    binary = root / ("probe.exe" if os.name == "nt" else "probe")
    subprocess.run(["go","build","-trimpath","-o",str(binary),str(source)],
                   cwd=root,env={**os.environ,"CGO_ENABLED":"0","GO111MODULE":"off"},check=True)
    return binary


def configure(case, root, env, probe):
    if case.raw_home:
        env["HOME"] = str(root / os.fsdecode(b"home\xff\xe2\x82"))
    legacy.setup_release_fixture(root, env)
    if case.verifier:
        verifier = Path(env["PATH"]) / ("cosign.exe" if os.name == "nt" else "cosign")
        shutil.copyfile(probe, verifier)
        verifier.chmod(0o755)
        env["DOCTOR_SIGNATURE_RECEIPT"] = str(root / "signature-invocations")
    for key, value in case.env:
        env[key] = value
    if case.config is not None:
        path = Path(env["XDG_CONFIG_HOME"]) / "symbrain/config.toml"
        path.parent.mkdir(parents=True)
        path.write_text(case.config)
    if case.project_config is not None:
        (Path(env["PROJECT"]) / ".symbrain.toml").write_text(case.project_config)
    if case.home_mode:
        home_key = "USERPROFILE" if os.name == "nt" else "HOME"
        if case.home_mode == "unset":env.pop(home_key, None)
        else:env[home_key] = ""
        if case.home_fallback:
            fallback = root / "fallback-owner";fallback.mkdir()
            drive, tail = os.path.splitdrive(str(fallback))
            assert drive and tail, "Windows fallback fixture must have a real owned drive/path"
            env.update(HOMEDRIVE=drive, HOMEPATH=tail)
    if case.all_missing:
        return
    bin_dir = Path(env["HOME"]) / ".symaira/bin"
    bin_dir.mkdir(parents=True)
    for name, core in legacy._managed_cores().items():
        path = bin_dir / name
        shutil.copyfile(probe, path)
        path.chmod(0o644 if name == "symdesk" and case.nonexecutable else 0o755)
        if name == "symdesk" and case.signal:
            path.write_text(f"#!/bin/sh\nkill -{case.signal} $$\n")
        if os.name == "nt":
            shutil.copyfile(probe, path.with_suffix(".exe"))
        version = json.dumps({"version":legacy._version_without_tag(core["version"])}).encode()
        response = {"output":base64.b64encode(case.version if name == "symdesk" and case.version is not None else version).decode(),
                    "exit":case.probe_exit if name == "symdesk" else 0,
                    "wait_ms":case.probe_wait if name == "symdesk" else 0}
        path.with_name(name + ".probe.json").write_text(json.dumps(response,sort_keys=True))
        if name == "symdesk" and case.provenance is not None:
            sidecar = path.with_name(name + ".provenance.json")
            if case.provenance == "directory":
                sidecar.mkdir()
            else:
                sidecar.write_bytes(case.provenance)


def filesystem(root, originals, started, ended):
    result = {}
    for path in sorted(root.rglob("*")):
        relative = path.relative_to(root).as_posix()
        mode = stat.S_IMODE(path.lstat().st_mode)
        if path.is_symlink():
            result[relative] = {"type":"link","mode":mode,"target":str(path.readlink())}
        elif path.is_dir():
            result[relative] = {"type":"dir","mode":mode}
        else:
            content = legacy.normalize_fixture_root(path.read_bytes(), root)
            if relative.endswith(".provenance.json") and content != originals.get(relative):
                value = json.loads(content)
                assert value["source"] == "release", "unexpected origin mutation"
                timestamp = value["built_at"]
                observed = legacy.datetime.fromisoformat(timestamp.replace("Z","+00:00")).timestamp()
                assert timestamp.endswith("Z") and started - 1 <= observed <= ended + 1
                content = content.replace(timestamp.encode(),b"<install-timestamp>")
            result[relative] = {"type":"file","mode":mode,"sha256":hashlib.sha256(content).hexdigest(),
                                "size":len(content)}
            if len(content) < 4096:
                result[relative]["content_base64"] = base64.b64encode(content).decode()
    return result


def log_contract(stderr, root, started, ended, case):
    data = legacy.normalize_fixture_root(stderr, root)
    groups = {}
    final = []
    finished = False
    for line in data.splitlines(keepends=True):
        match = re.match(rb"^(\d{4}/\d{2}/\d{2} \d{2}:\d{2}:\d{2}) (INFO|WARN|ERROR) (.*)\n$",line)
        if not match:
            final.append(base64.b64encode(line).decode("ascii"))
            finished = True
            continue
        parsed = time.strptime(match[1].decode(),"%Y/%m/%d %H:%M:%S")
        # Unix children explicitly use TZ=UTC. Go and chrono on Windows
        # obtain the native OS timezone and do not use Unix's TZ override.
        observed = time.mktime(parsed) if os.name == "nt" else calendar.timegm(parsed)
        assert started - 1 <= observed <= ended + 1, "log timestamp outside actual run"
        body = match[2] + b" " + match[3] + b"\n"
        core = re.search(rb" binary=(sym[a-z]+)(?: |$)",body)
        if core:
            assert not finished, "core event follows completion"
            assert core[1].decode() in legacy._managed_cores(), "unknown logged core"
            # The two retained actual Go runs choose distinct CreateTemp names.
            # Require this owned raw-HOME fixture, exact bin dir/core/destination,
            # syscall and error; only the generated source basename can differ.
            if case.raw_home:
                bin_dir = rb"<root>/home\xff\xe2\x82/.symaira/bin/"
                prefix = (b'ERROR repair failed binary=' + core[1] +
                          b' error="managed: record provenance for ' + core[1] +
                          b': managed: rename provenance: rename ' + bin_dir + b'.provenance-')
                suffix = b' ' + bin_dir + core[1] + b'.provenance.json: file exists"\n'
                if re.fullmatch(re.escape(prefix) + rb"[A-Za-z0-9]+" + re.escape(suffix), body):
                    body = prefix + b"<unique>" + suffix
            groups.setdefault(core[1].decode(),[]).append(base64.b64encode(body).decode("ascii"))
        else:
            assert body.startswith(b"INFO doctor --fix complete "), "unattributed event"
            finished = True
            final.append(base64.b64encode(body).decode("ascii"))
    # Go ActiveCores is a map. Its core order varies in real processes, while
    # each core's sequence, every attribute, and the completion tail are exact.
    return {"encoding":"base64 exact line bytes","cores":groups,"tail":final}


def observe(binary, case, root, probe, go, control=None):
    env = legacy.prepare_root(root, go)
    configure(case, root, env, probe)
    env["SYMBRAIN_GO_BINARY"] = str(root / "absent-go-fallback")
    if control:
        env["DOCTOR_CONTROL_TARGET"], env["DOCTOR_CONTROL_MUTATION"] = control
    originals = {p.relative_to(root).as_posix():legacy.normalize_fixture_root(p.read_bytes(),root)
                 for p in root.rglob("*.provenance.json") if p.is_file()}
    started = time.time()
    before = filesystem(root, originals, started, started)
    completed = subprocess.run([str(binary),*case.args],cwd=env["PROJECT"],env=env,
                               capture_output=True,timeout=20)
    ended = time.time()
    contract = {"exit":completed.returncode,
                "stdout_base64":base64.b64encode(legacy.normalize_fixture_root(completed.stdout,root)).decode(),
                "logs":log_contract(completed.stderr,root,started,ended,case),
                "filesystem":filesystem(root, originals, started, ended)}
    return {"binary":str(binary),"fixture_root":str(root),"started":started,"ended":ended,
            "exit":completed.returncode,"stdout_base64":base64.b64encode(completed.stdout).decode(),
            "stderr_base64":base64.b64encode(completed.stderr).decode(),"filesystem_before":before,
            "contract":contract}


def main():
    if len(sys.argv) not in (4,5):
        raise SystemExit("usage: replay.py GO_BINARY RUST_BINARY REPORT_JSON [CONTROL]")
    go, rust, report = (Path(p).resolve() for p in sys.argv[1:4])
    control = sys.argv[4] if len(sys.argv) == 5 else None
    assert control in (None,"wrong-exit","missing-header","missing-core-event")
    legacy.ensure_external_environment(__file__)
    legacy._build_windows_stub()
    selected = cases() if not control else cases()[:1]
    assert len({c.name for c in selected}) == len(selected)
    observations = []
    failures = []
    with tempfile.TemporaryDirectory(prefix="doctor-repair-fixture-") as tmp, legacy.ReleaseFixtureServer() as url:
        tmp = Path(tmp).resolve()
        probe = build_probe(tmp)
        probe_hash = digest(probe)
        legacy.RELEASE_BASE_URL = url
        for index, case in enumerate(selected):
            with tempfile.TemporaryDirectory(prefix="doctor-repair-case-") as directory:
                directory = Path(directory).resolve()
                go_root, rust_root = directory / "go", directory / "rust"
                try:
                    left = observe(go,case,go_root,probe,go)
                    right = observe(probe if control else rust,case,rust_root,probe,go,
                                    (str(rust),control) if control else None)
                    matches = left["contract"] == right["contract"]
                    observations.append({"case":case.name,"args":case.args,"matches":matches,"go":left,"rust":right})
                    if not matches:
                        differences = [key for key in left["contract"] if left["contract"][key] != right["contract"][key]]
                        failures.append({"case":case.name,"fields":differences})
                        print("FAIL",case.name,differences)
                except Exception as error:
                    failures.append({"case":case.name,"error":str(error)})
                    print("FAIL",case.name,str(error))
    data = {"go_oracle_ref":ORACLE,"candidate_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),
            "candidate_dirty":bool(subprocess.check_output(["git","status","--porcelain"],cwd=ROOT)),
            "runtime":platform.platform(),"go_sdk":subprocess.check_output(["go","version"],text=True).strip(),
            "go_binary_sha256":digest(go),"rust_binary_sha256":digest(rust),"probe_binary_sha256":probe_hash,
            "total":len(selected),"matched":sum(o["matches"] for o in observations),
            "control":control,
            "complete_observations":len(observations),"exit":int(bool(failures)),"failures":failures,
            "observations":observations,
            "comparison":"exact stdout, exit, full fixture files/modes and all per-core log sequences/attributes plus completion tail. Actual Go-equivalent clock timestamp in run window (Unix fixture TZ=UTC; Windows native OS timezone); only known root paths, proven-random provenance rename source basenames and new verified-UTC release-sidecar timestamps normalized. Frozen Go ActiveCores map order permits only between-core reorder; no per-core log or failure is discarded.",
            "fallback":"every actual process has an absent SYMBRAIN_GO_BINARY; fixture PATH contains no Go CLI",
            "remaining_scope":"typed config failure diagnostics and setup source-build/module lifecycle remain Go-owned in #765"}
    names = subprocess.check_output(["git","ls-tree","-r","--name-only",ORACLE,"--","cmd/symbrain","internal/managed","internal/config","internal/xdg","go.mod","go.sum"],cwd=ROOT,text=True).splitlines()
    data["go_source_sha256"] = {name:hashlib.sha256(subprocess.check_output(["git","show",f"{ORACLE}:{name}"],cwd=ROOT)).hexdigest()
                                 for name in names if name.endswith(".go") or name in ("go.mod","go.sum")}
    names = subprocess.check_output(["git","diff","--name-only","origin/main"],cwd=ROOT,text=True).splitlines()
    names += [str(p.relative_to(ROOT)) for folder in (ROOT/"scripts/doctor-repair-oracle",ROOT/"rust/symbrain-managed/src") for p in folder.glob("*") if p.is_file()]
    data["candidate_source_sha256"] = {name:digest(ROOT/name) for name in sorted(set(names))
                                       if (ROOT/name).is_file() and not name.startswith("migration/evidence/")}
    report.write_text(json.dumps(data,indent=2)+"\n")
    print(f"Doctor repair: {data['matched']}/{data['total']} matched, {len(failures)} failures")
    return data["exit"]


if __name__ == "__main__":
    raise SystemExit(main())
