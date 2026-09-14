#!/usr/bin/env python3
"""Provenance-bound, bounded black-box harness for ``symbrain guard decide``."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import signal
import stat
import subprocess
import sys
import tempfile
import tarfile
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
CASE_FILE = HERE / "cases.json"
EVIDENCE = ROOT / "target" / "migration-run" / "guard-decide-provenancefix"
GO = Path(os.environ.get("GUARD_DECIDE_GO", "/Users/daniel/sdk/go1.26.6/bin/go")).resolve()
PINNED_COMMIT_SHA = "0b585d52915a824664e1377d0a995dff3f5405cd"
SELECTED_TOOLCHAIN = "go1.26.7"
MAX_REQUEST = 64 * 1024
BUILD_TIMEOUT = 180.0
CASE_TIMEOUT = 8.0
RESERVED_ENV = {"HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR", "TMP", "TEMP", "PATH", "LANG", "LC_ALL", "TZ"}


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def run_bounded(argv: list[str], *, cwd: Path, env: dict[str, str], stdin: bytes | None,
                stdout: int | None = subprocess.PIPE, stderr: int | None = subprocess.PIPE,
                timeout: float) -> subprocess.CompletedProcess[bytes]:
    """Run a process group with file-backed output and descendant cleanup."""
    out_file = tempfile.NamedTemporaryFile(prefix="oracle-out-", delete=False)
    err_file = tempfile.NamedTemporaryFile(prefix="oracle-err-", delete=False)
    out_name, err_name = out_file.name, err_file.name
    out_file.close(); err_file.close()
    try:
        with open(out_name, "wb") as out, open(err_name, "wb") as err:
            try:
                p = subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.PIPE if stdin is not None else None,
                                     stdout=out if stdout == subprocess.PIPE else stdout,
                                     stderr=err if stderr == subprocess.PIPE else stderr,
                                     start_new_session=True)
            except BaseException:
                raise
            timed_out = False
            try:
                p.communicate(input=stdin, timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                try:
                    os.killpg(p.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    p.wait(timeout=1.0)
                except subprocess.TimeoutExpired:
                    try:
                        os.killpg(p.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                    p.wait(timeout=2.0)
            rc = p.returncode
        result = subprocess.CompletedProcess(argv, rc, Path(out_name).read_bytes(), Path(err_name).read_bytes())
        result.timed_out = timed_out
        return result
    finally:
        for name in (out_name, err_name):
            try: os.unlink(name)
            except FileNotFoundError: pass


def git_tree(commit: str) -> tuple[list[str], dict[str, str]]:
    require_commit_sha(commit, "commit SHA")
    subprocess.run(["git", "cat-file", "-e", f"{commit}^{{commit}}"], cwd=ROOT, check=True)
    archive = subprocess.check_output(["git", "archive", "--format=tar", commit], cwd=ROOT)
    paths, hashes = [], {}
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tree:
        for member in tree:
            if not member.isfile():
                continue
            name = member.name
            content = tree.extractfile(member).read()
            paths.append(name); hashes[name] = digest(content)
    return paths, hashes


def tracked_tree() -> tuple[list[str], dict[str, str]]:
    paths, _ = git_tree(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip())
    return paths, {p: digest((ROOT / p).read_bytes()) for p in paths}


def evidence_path(root: Path, value: object, label: str) -> Path:
    if not isinstance(value, str) or not value or Path(value).is_absolute():
        raise AssertionError(f"{label} must be a relative evidence path")
    relative = Path(value)
    if any(part in ("..", "") for part in relative.parts):
        raise AssertionError(f"{label} escapes evidence root")
    root = root.resolve()
    resolved = (root / relative).resolve(strict=False)
    if resolved != root and root not in resolved.parents:
        raise AssertionError(f"{label} escapes evidence root")
    return resolved


def require_commit_sha(value: object, label: str = "commit SHA") -> str:
    if not isinstance(value, str) or re.fullmatch(r"[0-9a-f]{40}", value) is None:
        raise AssertionError(f"{label} must be a full 40-character lowercase hex SHA")
    return value


def provenance(extracted: Path, *, expected_commit_sha: str = PINNED_COMMIT_SHA) -> dict[str, object]:
    expected_commit_sha = require_commit_sha(expected_commit_sha, "expected commit SHA")
    paths, expected_hashes = git_tree(expected_commit_sha)
    extract_hashes = {p: digest((extracted / p).read_bytes()) for p in paths if (extracted / p).is_file()}
    if extract_hashes != expected_hashes:
        raise RuntimeError("extracted pinned tree does not exactly match trusted commit tree")
    head = require_commit_sha(subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(), "HEAD")
    return {"commit_sha": expected_commit_sha, "current_head": head,
            "tracked_files": [{"path": p, "sha256": expected_hashes[p], "bytes": (extracted / p).stat().st_size} for p in paths],
            "source_before": expected_hashes, "source_after": expected_hashes}


def raw_input(case: dict[str, object], launch: str) -> bytes:
    if "raw" in case: return str(case["raw"]).encode()
    if "raw_prefix" in case:
        return (str(case["raw_prefix"]) + str(case.get("raw_repeat", "")) * int(case["raw_repeat_count"]) + str(case["raw_suffix"])).encode()
    req = dict(case["request"])
    if req.get("deadline") == "__LAUNCH_RFC3339__": req["deadline"] = launch
    return (json.dumps(req, separators=(",", ":")) + "\n").encode()


def validate_response(case: dict[str, object], stdout: bytes) -> dict[str, object]:
    if case.get("expect_process_failure"): return {}
    try: response = json.loads(stdout)
    except json.JSONDecodeError as exc: raise AssertionError(f"invalid JSON response: {exc}")
    if response.get("decision") != case.get("decision"): raise AssertionError("decision differs from case")
    reason = response.get("reason")
    if not isinstance(reason, str) or not reason: raise AssertionError("missing response reason")
    needle = case.get("reason_contains")
    if needle and needle not in reason: raise AssertionError("response reason differs from case")
    return response


def validate_audit(runtime: Path, response: dict[str, object], case_root: Path) -> dict[str, object]:
    log = runtime / "data" / "symguard" / "audit.log"
    if not log.is_file(): raise AssertionError("audit log missing")
    dmode = stat.S_IMODE(log.parent.stat().st_mode); fmode = stat.S_IMODE(log.stat().st_mode)
    if dmode != 0o700 or fmode != 0o600: raise AssertionError("audit permissions are not 0700/0600")
    lines = log.read_bytes().splitlines()
    if len(lines) != 1: raise AssertionError("audit JSONL must contain one record")
    record = json.loads(lines[0])
    required = {"id", "command", "decision", "reason", "decided_at"}
    if not required.issubset(record) or set(record) - required - {"risk_class", "domain", "warnings"} or record["decision"] != response.get("decision"): raise AssertionError("audit structure/decision mismatch")
    if any(secret in lines[0].lower().decode("utf-8", errors="replace") for secret in ("password", "token", "secret", "api_key")): raise AssertionError("audit contains unredacted secret material")
    saved = case_root / "audit.jsonl"; saved.write_bytes(log.read_bytes()); os.chmod(saved, 0o600)
    return {"path": str(saved.relative_to(EVIDENCE)), "bytes": saved.stat().st_size, "sha256": digest(saved.read_bytes()), "fields": sorted(record), "dir_mode": oct(dmode), "file_mode": oct(fmode), "decision": record["decision"]}


def toolchain_metadata(binary: Path, build_env: dict[str, str]) -> dict[str, object]:
    launcher_env = dict(build_env)
    launcher_env["GOTOOLCHAIN"] = "local"
    launcher_version = subprocess.check_output([str(GO), "version"], text=True, env=launcher_env).strip()
    buildinfo = subprocess.check_output([str(GO), "version", "-m", str(binary)], text=True, env=build_env)
    first_line = buildinfo.splitlines()[0]
    match = re.search(r": (go[0-9]+\.[0-9]+\.[0-9]+)(?: |$)", first_line)
    if match is None: raise RuntimeError("built binary has no parseable Go build toolchain")
    selected_version = match.group(1)
    goroot = Path(subprocess.check_output([str(GO), "env", "GOROOT"], text=True, env=build_env).strip()).resolve()
    selected_executable = goroot / "bin" / "go"
    if not selected_executable.is_file(): raise RuntimeError("selected Go toolchain executable is missing")
    executable_version = subprocess.check_output([str(selected_executable), "version"], text=True, env=build_env).strip()
    if selected_version not in executable_version: raise RuntimeError("selected Go executable does not match built binary toolchain")
    if selected_version != SELECTED_TOOLCHAIN: raise RuntimeError(f"built binary selected {selected_version}, want {SELECTED_TOOLCHAIN}")
    return {"launcher_path": str(GO), "launcher_version": launcher_version, "launcher_sha256": digest(GO.read_bytes()), "selected_version": selected_version, "selected_goroot": str(goroot), "selected_executable": str(selected_executable), "selected_executable_sha256": digest(selected_executable.read_bytes()), "buildinfo_first_line": first_line}


def run_case(binary: Path, case: dict[str, object], index: int, run_root: Path, launch: str, cwd: Path, base_env: dict[str, str]) -> dict[str, object]:
    case_root = run_root / f"{index:02d}-{case['id']}"; case_root.mkdir()
    runtime = Path(tempfile.mkdtemp(prefix="symbrain-guard-decide-", dir="/private/tmp"))
    try:
        roots = {name: runtime / name for name in ("home", "data", "config", "cache", "tmp")}
        for p in roots.values(): p.mkdir()
        env = dict(base_env); env.update({"HOME": str(roots["home"]), "XDG_DATA_HOME": str(roots["data"]), "XDG_CONFIG_HOME": str(roots["config"]), "XDG_CACHE_HOME": str(roots["cache"]), "TMPDIR": str(roots["tmp"])})
        if case["id"] == "audit-failure":
            bad = case_root / "data-is-a-file"; bad.write_bytes(b"not a directory"); env["XDG_DATA_HOME"] = str(bad)
        payload = raw_input(case, launch)
        argv = [str(binary), "guard", "decide"]
        if case.get("expect_process_failure"):
            sink = case_root / "read-only-stdout"; sink.write_bytes(b"")
            with sink.open("rb") as fd: proc = run_bounded(argv, cwd=cwd, env=env, stdin=payload, stdout=fd, timeout=CASE_TIMEOUT)
        else: proc = run_bounded(argv, cwd=cwd, env=env, stdin=payload, timeout=CASE_TIMEOUT)
        if case.get("expect_process_failure"):
            if proc.returncode != 1: raise AssertionError(f"output failure exited {proc.returncode}")
            response = {}
        else:
            if proc.returncode != 0: raise AssertionError(f"command exited {proc.returncode}: {proc.stderr.decode(errors='replace')}")
            response = validate_response(case, proc.stdout)
        out = case_root / "stdout.bin"; err = case_root / "stderr.bin"; out.write_bytes(proc.stdout); err.write_bytes(proc.stderr)
        audit = None
        if not case.get("expect_process_failure") and case["id"] != "audit-failure": audit = validate_audit(runtime, response, case_root)
        result = {"id": case["id"], "argv": argv, "cwd": str(cwd), "exit_code": proc.returncode, "timed_out": bool(getattr(proc, "timed_out", False)), "request_bytes": len(payload), "request_sha256": digest(payload), "stdout": {"path": str(out.relative_to(EVIDENCE)), "bytes": len(proc.stdout), "sha256": digest(proc.stdout)}, "stderr": {"path": str(err.relative_to(EVIDENCE)), "bytes": len(proc.stderr), "sha256": digest(proc.stderr)}, "expected": {k: case[k] for k in ("decision", "reason_contains") if k in case}}
        if audit: result["audit"] = audit
        return result
    finally:
        shutil.rmtree(runtime, ignore_errors=True)


def validate_manifest(path: Path, *, cases_path: Path = CASE_FILE, expected_digest: str | None = None, expected_commit_sha: str = PINNED_COMMIT_SHA) -> dict[str, object]:
    manifest = json.loads(path.read_text())
    if manifest.get("schema_version") != 2: raise AssertionError("unsupported manifest schema")
    raw_manifest = path.read_bytes()
    if expected_digest and digest(raw_manifest) != expected_digest: raise AssertionError("manifest digest mismatch")
    cases_doc = json.loads(cases_path.read_text())
    cases = cases_doc["cases"]
    if manifest["cases"]["sha256"] != digest(cases_path.read_bytes()): raise AssertionError("case manifest hash mismatch")
    if manifest["harness"]["sha256"] != digest(Path(__file__).read_bytes()): raise AssertionError("harness hash mismatch")
    source = manifest["source_identity"]
    expected_commit_sha = require_commit_sha(expected_commit_sha, "expected commit SHA")
    if require_commit_sha(source.get("commit_sha")) != expected_commit_sha: raise AssertionError("source commit is not the pinned acceptance revision")
    trusted_paths, trusted_hashes = git_tree(expected_commit_sha)
    listed = {item["path"]: item["sha256"] for item in source["tracked_files"]}
    if trusted_paths != list(listed): raise AssertionError("trusted historical tree inventory mismatch")
    if listed != trusted_hashes or source["source_before"] != trusted_hashes or source["source_after"] != trusted_hashes: raise AssertionError("historical source hash mismatch")
    for item in source["tracked_files"]:
        if type(item.get("bytes")) is not int: raise AssertionError("historical source byte metadata mismatch")
        actual = subprocess.check_output(["git", "show", f"{expected_commit_sha}:{item['path']}"], cwd=ROOT)
        if len(actual) != item["bytes"]: raise AssertionError("historical source byte count mismatch")
    binary = evidence_path(EVIDENCE, manifest["binary"]["path"], "binary path")
    binary_bytes = binary.read_bytes() if binary.is_file() else b""
    if not binary.is_file() or binary.stat().st_size != manifest["binary"]["bytes"] or len(binary_bytes) != manifest["binary"]["bytes"] or digest(binary_bytes) != manifest["binary"]["sha256"]: raise AssertionError("binary hash/size mismatch")
    diagnostic_cases = [c for c in cases if c["id"] == "deadline-equality-at-launch"]
    acceptance_cases = [c for c in cases if c["id"] != "deadline-equality-at-launch"]
    results = manifest.get("results")
    if not isinstance(results, list) or manifest.get("case_count") != len(cases) or manifest.get("diagnostic_count") != len(diagnostic_cases) or manifest.get("acceptance_case_count") != len(acceptance_cases) or len(results) != len(cases) or [r.get("id") for r in results] != [c["id"] for c in cases]: raise AssertionError("case inventory/count mismatch")
    if manifest["source_identity"]["source_before"] != manifest["source_identity"]["source_after"]: raise AssertionError("source changed during run")
    expected_toolchain = toolchain_metadata(binary, {"PATH": "/usr/bin:/bin", "HOME": str(EVIDENCE / "build-home"), "GOTOOLCHAIN": SELECTED_TOOLCHAIN, "CGO_ENABLED": "0", "TMPDIR": str(EVIDENCE / "build-tmp")})
    if manifest.get("toolchain") != expected_toolchain | {"CGO_ENABLED": "0", "GOTOOLCHAIN": SELECTED_TOOLCHAIN}: raise AssertionError("toolchain metadata mismatch")
    for r, c in zip(results, cases):
        if not isinstance(r, dict) or not isinstance(r.get("id"), str) or r["id"] != c["id"]: raise AssertionError("result id mismatch")
        if type(r.get("exit_code")) is not int or r["exit_code"] != (1 if c.get("expect_process_failure") else 0): raise AssertionError("exit code mismatch")
        if type(r.get("timed_out")) is not bool or r["timed_out"]: raise AssertionError("timeout metadata mismatch")
        expected = {k: c[k] for k in ("decision", "reason_contains") if k in c}
        if r.get("expected") != expected: raise AssertionError("expected case binding mismatch")
        if not isinstance(r.get("request_bytes"), int) or not isinstance(r.get("request_sha256"), str): raise AssertionError("request metadata mismatch")
        for key in ("stdout", "stderr"):
            meta = r.get(key)
            if not isinstance(meta, dict) or type(meta.get("bytes")) is not int or not isinstance(meta.get("sha256"), str): raise AssertionError(f"{key} metadata mismatch")
            p = evidence_path(EVIDENCE, meta.get("path"), f"{key} path")
            data = p.read_bytes() if p.is_file() else b""
            if not p.is_file() or len(data) != meta["bytes"] or p.stat().st_size != meta["bytes"] or digest(data) != meta["sha256"]: raise AssertionError(f"{key} evidence hash/size mismatch")
        wants_audit = not c.get("expect_process_failure") and c["id"] != "audit-failure"
        if ("audit" in r) != wants_audit: raise AssertionError("audit presence mismatch")
        if "audit" in r:
            meta = r["audit"]
            if not isinstance(meta, dict) or type(meta.get("bytes")) is not int or not isinstance(meta.get("sha256"), str): raise AssertionError("audit metadata mismatch")
            p = evidence_path(EVIDENCE, meta.get("path"), "audit path")
            data = p.read_bytes() if p.is_file() else b""
            if not p.is_file() or len(data) != meta["bytes"] or p.stat().st_size != meta["bytes"] or digest(data) != meta["sha256"]: raise AssertionError("audit evidence hash/size mismatch")
    return manifest


def main() -> int:
    ap = argparse.ArgumentParser(); ap.add_argument("--check", action="store_true"); ap.add_argument("--validate", type=Path); ap.add_argument("--expected-manifest-digest"); ap.add_argument("--expected-commit-sha", default=PINNED_COMMIT_SHA); args = ap.parse_args()
    if args.validate: validate_manifest(args.validate, expected_digest=args.expected_manifest_digest, expected_commit_sha=args.expected_commit_sha); print(json.dumps({"status": "valid", "manifest": str(args.validate)})); return 0
    if not GO.is_file(): raise RuntimeError(f"Go toolchain not found: {GO}")
    require_commit_sha(PINNED_COMMIT_SHA, "pinned commit SHA")
    EVIDENCE.mkdir(parents=True, exist_ok=True); extracted = EVIDENCE / "pinned-tree"; shutil.rmtree(extracted, ignore_errors=True); extracted.mkdir()
    archive = subprocess.run(["git", "archive", "--format=tar", PINNED_COMMIT_SHA], cwd=ROOT, stdout=subprocess.PIPE, check=True)
    subprocess.run(["tar", "-xf", "-", "-C", str(extracted)], input=archive.stdout, check=True)
    identity = provenance(extracted, expected_commit_sha=PINNED_COMMIT_SHA)
    build_dir = EVIDENCE / "build"; build_dir.mkdir(exist_ok=True); binary = build_dir / "symbrain-go"
    build_env = {"PATH": "/usr/bin:/bin", "HOME": str(EVIDENCE / "build-home"), "GOTOOLCHAIN": "go1.26.7", "CGO_ENABLED": "0", "TMPDIR": str(EVIDENCE / "build-tmp")}
    Path(build_env["HOME"]).mkdir(exist_ok=True); Path(build_env["TMPDIR"]).mkdir(exist_ok=True)
    build = run_bounded([str(GO), "build", "-o", str(binary), "./cmd/symbrain"], cwd=extracted, env=build_env, stdin=None, timeout=BUILD_TIMEOUT)
    (EVIDENCE / "build.stdout").write_bytes(build.stdout); (EVIDENCE / "build.stderr").write_bytes(build.stderr)
    if build.returncode != 0: raise RuntimeError(f"Go build failed with exit {build.returncode}")
    run_root = EVIDENCE / "cases"; shutil.rmtree(run_root, ignore_errors=True); run_root.mkdir()
    cases_doc = json.loads(CASE_FILE.read_text()); launch = datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")
    base_env = {"PATH": "/usr/bin:/bin", "LANG": "C", "LC_ALL": "C", "TZ": "UTC"}
    results = [run_case(binary, c, i, run_root, launch, extracted, base_env) for i, c in enumerate(cases_doc["cases"], 1)]
    manifest = {"schema_version": 2, "source_identity": identity, "harness": {"path": str(HERE.relative_to(ROOT) / "oracle.py"), "sha256": digest(Path(__file__).read_bytes())}, "cases": {"path": str(CASE_FILE.relative_to(ROOT)), "sha256": digest(CASE_FILE.read_bytes())}, "toolchain": toolchain_metadata(binary, build_env) | {"CGO_ENABLED": "0", "GOTOOLCHAIN": SELECTED_TOOLCHAIN}, "binary": {"path": str(binary.relative_to(EVIDENCE)), "bytes": binary.stat().st_size, "sha256": digest(binary.read_bytes())}, "argv_template": ["symbrain", "guard", "decide"], "environment_allowlist": sorted(set(base_env) | {"HOME", "XDG_DATA_HOME", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "TMPDIR"}), "launch_timestamp": launch, "case_count": len(results), "diagnostic_count": sum(c["id"] == "deadline-equality-at-launch" for c in cases_doc["cases"]), "acceptance_case_count": len(results) - sum(c["id"] == "deadline-equality-at-launch" for c in cases_doc["cases"]), "results": results}
    mp = EVIDENCE / "manifest.json"; mp.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n"); validate_manifest(mp, expected_commit_sha=PINNED_COMMIT_SHA)
    print(json.dumps({"status": "pass", "case_count": len(results), "diagnostic_count": manifest["diagnostic_count"], "manifest": str(mp)}, sort_keys=True)); return 0


if __name__ == "__main__":
    try: raise SystemExit(main())
    except (AssertionError, OSError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as exc: print(f"guard-decide-oracle: {exc}", file=sys.stderr); raise SystemExit(1)
