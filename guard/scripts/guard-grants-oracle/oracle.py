#!/usr/bin/env python3
"""Provenance-bound black-box oracle for ``symbrain guard grants``."""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(ROOT / "scripts"))
from external_env import ensure_external_environment as ensure_shared_external_environment

FIXTURE = HERE / "fixture.json"
PINNED_COMMIT_SHA = "9c0e2b259753901a372ed5a688382bb6d4fadd18"
GO_SOURCE_PATHS = (
    "cmd/symbrain/cmd_guard.go",
    "cmd/symbrain/main.go",
    "guard/cmd/symguard/grants/command.go",
    "guard/internal/config/config.go",
    "guard/internal/grant/grant.go",
    "guard/internal/grant/store.go",
    "go.mod",
    "go.sum",
)
GO_TOOLCHAIN = "go1.26.7"


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def ensure_external_environment() -> None:
    ensure_shared_external_environment(__file__)


def git_show(path: str) -> bytes:
    return subprocess.check_output(["git", "show", f"{PINNED_COMMIT_SHA}:{path}"], cwd=ROOT)


def verify_binding(document: dict[str, object]) -> dict[str, str]:
    try:
        subprocess.run(["git", "cat-file", "-e", f"{PINNED_COMMIT_SHA}^{{commit}}"], cwd=ROOT,
                       check=True, stdout=subprocess.DEVNULL)
        expected = {path: sha256(git_show(path)) for path in GO_SOURCE_PATHS}
    except (OSError, subprocess.CalledProcessError) as exc:
        raise RuntimeError(f"pinned Go revision unavailable: {PINNED_COMMIT_SHA}") from exc
    if document.get("source_commit") != PINNED_COMMIT_SHA:
        raise RuntimeError("fixture is not bound to the pinned Go revision")
    if document.get("source_files") != expected:
        raise RuntimeError("fixture source hashes do not match the pinned Go revision")
    current = {path: sha256((ROOT / path).read_bytes()) for path in GO_SOURCE_PATHS}
    if current != expected:
        raise RuntimeError("working-tree Go grants sources drifted from the pinned revision")
    return expected


def run(argv: list[str], *, cwd: Path, env: dict[str, str], timeout: float = 180.0) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(argv, cwd=cwd, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          timeout=timeout, check=False)


def encoded(data: bytes) -> dict[str, object]:
    return {"base64": base64.b64encode(data).decode("ascii"), "bytes": len(data), "sha256": sha256(data)}


def canonical(data: bytes, runtime: Path) -> bytes:
    return data.replace(str(runtime).encode(), b"/oracle-runtime")


def grant(grant_id: str, subject: str, scope: str, granted_at: str) -> dict[str, object]:
    return {
        "id": grant_id,
        "scope": scope,
        "origin": {"epoch": 1722924000, "via": "approval"},
        "granted_at": granted_at,
        "subject": subject,
        "capability": "read_private",
        "purpose": "oracle-purpose",
        "resource": "fs/read_file",
        "scope_ceiling": ["session"],
        "expires_at": "2027-01-01T00:00:00Z",
    }


def seed(runtime: Path, case_id: str) -> None:
    data = runtime / "data"
    entries: list[dict[str, object]] | None = None
    if case_id == "list-ordered":
        entries = [
            grant("gnt-old", "agent-old", "device", "2026-08-06T10:00:00Z"),
            grant("gnt-new", "agent-new", "vault", "2026-08-06T11:00:00Z"),
        ]
    elif case_id == "revoke-id":
        entries = [
            grant("gnt-keep", "agent-keep", "device", "2026-08-06T10:00:00Z"),
            grant("gnt-revoke", "agent-revoke", "vault", "2026-08-06T11:00:00Z"),
        ]
    elif case_id == "revoke-all":
        entries = [
            grant("gnt-a", "agent-a", "device", "2026-08-06T10:00:00Z"),
            grant("gnt-b", "agent-b", "vault", "2026-08-06T11:00:00Z"),
        ]
    elif case_id == "list-fractional-offset":
        entries = [
            grant("gnt-fractional", "agent-fractional", "device", "2026-08-06T16:34:56.123456789Z"),
            grant("gnt-offset", "agent-offset", "vault", "2026-08-06T12:34:56.789-04:00"),
        ]
    elif case_id == "string-escaping":
        special = "<>&/\u2028\u2029"
        entry = grant("gnt-string", "agent-string", "device", "2026-08-06T10:00:00Z")
        entry.update({
            "subject": special,
            "capability": special,
            "purpose": special,
            "resource": special,
            "scope_ceiling": [special],
            "origin": {"epoch": 1722924000, "via": special},
        })
        entries = [entry]
    if entries is None and case_id not in {
        "malformed", "legacy-list", "legacy-revoke", "top-level-null", "legacy-nullable"
    }:
        return
    data.mkdir(parents=True, mode=0o700, exist_ok=True)
    if case_id == "malformed":
        (data / "grants.json").write_bytes(b"{not json\n")
    elif case_id == "top-level-null":
        (data / "grants.json").write_bytes(b"null\n")
    elif case_id == "legacy-nullable":
        (data / "grants.json").write_bytes(
            b'[{"id":"legacy-nullable","scope":"device","origin":null,'
            b'"granted_at":null,"subject":"agent-nullable","capability":null,'
            b'"purpose":null,"resource":null,"scope_ceiling":[null],'
            b'"expires_at":null,"revoked":null},'
            b'{"id":"legacy-missing","scope":"vault","subject":"agent-missing"}]'
        )
    elif case_id in {"legacy-list", "legacy-revoke"}:
        (data / "grants.json").write_bytes(
            b'[{"id":"legacy","scope":"device","subject":"agent-legacy",'
            b'"granted_at":"2026-08-06T10:00:00Z"}]'
        )
    if entries is not None:
        (data / "grants.json").write_text(json.dumps(entries, indent=2) + "\n", encoding="utf-8")
    os.chmod(data / "grants.json", 0o600)


def state(runtime: Path) -> dict[str, object]:
    data = runtime / "data"
    if not data.exists():
        return {"files": [], "directories": []}
    files = []
    for path in sorted(p for p in data.rglob("*") if p.is_file()):
        raw = path.read_bytes()
        files.append({
            "path": str(path.relative_to(data)),
            "mode": format(stat.S_IMODE(path.stat().st_mode), "04o"),
            "content": encoded(raw),
        })
    dirs = []
    for path in sorted([data, *[p for p in data.rglob("*") if p.is_dir()]], key=lambda p: str(p)):
        dirs.append({"path": "." if path == data else str(path.relative_to(data)),
                     "mode": format(stat.S_IMODE(path.stat().st_mode), "04o")})
    return {"files": files, "directories": dirs}


def run_case(binary: Path, source: Path, runtime: Path, case_id: str, args: list[str]) -> dict[str, object]:
    case_root = runtime / case_id
    case_root.mkdir()
    seed(case_root, case_id)
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "HOME": str(case_root / "home"),
        "SYMGUARD_DATA": str(case_root / "data"),
    }
    (case_root / "home").mkdir()
    proc = run([str(binary), "guard", "grants", *args], cwd=source, env=env, timeout=15.0)
    stdout = canonical(proc.stdout, case_root)
    stderr = canonical(proc.stderr, case_root)
    return {
        "id": case_id,
        "args": args,
        "exit_code": proc.returncode,
        "stdout": encoded(stdout),
        "stderr": encoded(stderr),
        "state": state(case_root),
    }


def build_and_run() -> dict[str, object]:
    expected = {path: sha256(git_show(path)) for path in GO_SOURCE_PATHS}
    runtime_parent = Path(os.environ.get("SYMAIRA_EXTERNAL_RUNTIME_ROOT", tempfile.gettempdir()))
    runtime = Path(tempfile.mkdtemp(prefix="guard-grants-oracle-", dir=runtime_parent))
    source = runtime / "source"
    binary = runtime / "symbrain-go"
    try:
        subprocess.run(["git", "worktree", "add", "--quiet", "--detach", str(source), PINNED_COMMIT_SHA],
                       cwd=ROOT, check=True)
        build_env = dict(os.environ)
        build_env.update({"HOME": str(runtime / "build-home"), "GOTOOLCHAIN": GO_TOOLCHAIN,
                          "CGO_ENABLED": "0", "TMPDIR": str(runtime / "build-tmp")})
        (runtime / "build-home").mkdir()
        (runtime / "build-tmp").mkdir()
        build = run(["go", "build", "-o", str(binary), "./cmd/symbrain"], cwd=source, env=build_env)
        if build.returncode != 0:
            raise RuntimeError(f"pinned Go build failed: {build.stderr.decode(errors='replace')}")
        cases = [
            run_case(binary, source, runtime, "list-empty", ["list"]),
            run_case(binary, source, runtime, "list-ordered", ["list"]),
            run_case(binary, source, runtime, "list-fractional-offset", ["list"]),
            run_case(binary, source, runtime, "revoke-id", ["revoke", "gnt-revoke"]),
            run_case(binary, source, runtime, "revoke-all", ["revoke", "--all"]),
            run_case(binary, source, runtime, "revoke-missing", ["revoke", "missing"]),
            run_case(binary, source, runtime, "malformed", ["list"]),
            run_case(binary, source, runtime, "top-level-null", ["list"]),
            run_case(binary, source, runtime, "legacy-list", ["list"]),
            run_case(binary, source, runtime, "legacy-revoke", ["revoke", "legacy"]),
            run_case(binary, source, runtime, "legacy-nullable", ["list"]),
            run_case(binary, source, runtime, "string-escaping", ["revoke", "gnt-string"]),
            run_case(binary, source, runtime, "usage-all-with-id", ["revoke", "--all", "gnt-a"]),
        ]
        return {"schema_version": 1, "source_commit": PINNED_COMMIT_SHA,
                "source_files": expected, "case_count": len(cases), "cases": cases}
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(source)], cwd=ROOT,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, check=False)
        shutil.rmtree(runtime, ignore_errors=True)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=("write", "check"))
    args = parser.parse_args()
    ensure_external_environment()
    if args.action == "check":
        current = json.loads(FIXTURE.read_text(encoding="utf-8"))
        verify_binding(current)
    generated = build_and_run()
    if args.action == "check":
        if current != generated:
            raise RuntimeError("guard grants oracle fixture drifted; run run.sh write after reviewing the source change")
        print(f"PASS: guard grants oracle byte/state check passed ({generated['case_count']} cases)")
    else:
        FIXTURE.write_text(json.dumps(generated, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        print(f"Wrote {FIXTURE} ({generated['case_count']} cases)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"guard-grants-oracle: {exc}", file=os.sys.stderr)
        raise SystemExit(1)
