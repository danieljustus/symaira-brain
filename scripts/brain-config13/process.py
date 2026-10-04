#!/usr/bin/env python3
"""Owned actual-process gate. Prepared, not executed, in the source checkpoint.

Every output byte, exit, filesystem entry and owned release request is compared.
The disabled-profile Memory/JWT difference remains a failure, not a projection.
The original census and its provisional attempts remain separate immutable data.
"""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PROFILE = b'''[profile]
name="owned"
[servers.vault]
enabled=false
[servers.memory]
enabled=false
[servers.skills]
enabled=false
[servers.usage]
enabled=false
[audit]
enabled=false
'''
FIELDS = [
    ("default_profile", b"default_profile = 7\n"),
    ("audit.enabled", b'[audit]\nenabled = "yes"\n'),
    ("audit.verbose", b"[audit]\nverbose = 1\n"),
    ("gateway.identity_injection", b'[gateway]\nidentity_injection = "yes"\n'),
    ("updatecheck.enabled", b"[updatecheck]\nenabled = []\n"),
    ("servers.vault.binary_path", b"[servers.vault]\nbinary_path = 1\n"),
    ("servers.operate.binary_path", b"[servers.operate]\nbinary_path = true\n"),
    ("servers.scope.binary_path", b'[servers.scope]\nbinary_path = ["x"]\n'),
    ("patterns.enabled", b"[patterns]\nenabled = {}\n"),
    ("patterns.promotion_threshold", b'[patterns]\npromotion_threshold = "9223372036854775808"\n'),
    ("modules.browse", b"[modules]\nbrowse = 1\n"),
    ("modules.operate", b'[modules]\noperate = "yes"\n'),
    ("modules.scope", b"[modules]\nscope = [false]\n"),
]


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def encoded(value):
    return base64.b64encode(value).decode("ascii")


def cases():
    for field, invalid in FIELDS:
        content = (b'' if field == "default_profile" else b'default_profile="owned"\n') + invalid
        for command in ("install", "mcp", "setup", "setup-fix", "doctor-fix", "source"):
            yield dict(id=f"{field}-{command}", command=command, global_bytes=content)
    for name, command, global_bytes, project_bytes, env in (
        ("project-default", "install", b'', b'default_profile="owned"\n', {}),
        ("project-over-global", "install", b'default_profile="global"\n', b'default_profile="owned"\n', {}),
        ("global-default", "install", b'default_profile="owned"\n', b'', {}),
        ("stored-map-bypass", "config-get", b'[audit]\nenabled=true\n', b'[audit]\nenabled="bad"\n', {"SYMBRAIN_AUDIT_ENABLED": "bad"}),
        ("declaration-order", "mcp", b'[modules]\nscope="bad"\n[audit]\nenabled="bad"\n', b'', {}),
        ("global-before-project", "mcp", b'[audit]\nenabled="globalbad"\n', b'[audit]\nenabled="projectbad"\n', {}),
        ("project-before-env", "mcp", b'', b'[audit]\nverbose="projectbad"\n', {"SYMBRAIN_AUDIT_ENABLED": "envbad"}),
        ("env-declaration-order", "mcp", b'', b'', {"SYMBRAIN_MODULES_SCOPE": "bad", "SYMBRAIN_AUDIT_ENABLED": "bad"}),
        ("plain-pointer-false", "mcp", b'[audit]\nenabled=false\nverbose=false\n[modules]\nbrowse=false\n', b'', {}),
        ("unknown-scalar-struct", "mcp", b'audit="ignored"\nmodules=[]\nunknown=true\n', b'', {}),
        ("bool-aliases", "mcp", b'[audit]\nenabled="FALSE"\n[gateway]\nidentity_injection="0"\n', b'', {}),
        ("empty-env", "install", b'default_profile="owned"\n', b'', {"SYMBRAIN_DEFAULT_PROFILE": ""}),
    ):
        yield dict(id=name, command=command, global_bytes=global_bytes,
                   project_bytes=project_bytes, env=env)
    yield dict(id="explicit-profile-bypass", command="install",
               global_bytes=b'[audit]\nenabled="bad"\n', extra_args=["--profile", "owned"])
    # Parser admission corpus is intentionally strict: no Rust-message adapter.
    for name, content in (
        ("original-bad-table", b"[bad config"),
        ("missing-array-end", b"invalid=[unterminated\n"),
        ("invalid-utf8", b'default_profile="owned"\n#\xff\n'),
        ("grammar-before-utf8", b"x=[unterminated\n#\xff\n"),
        ("duplicate-known", b'default_profile="first"\ndefault_profile="second"\n'),
        ("redeclared-table", b"[audit]\nenabled=true\n[audit]\nverbose=true\n"),
        ("multiline-literal", b"default_profile='''owned'''\n"),
        ("dotted-known", b"audit.enabled=false\nmodules.browse=true\n"),
        ("offset-datetime-wrong-type", b"default_profile=1979-05-27T07:32:00Z\n"),
        ("numeric-base-wrong-type", b"default_profile=0x10\n"),
        ("array-table-wrong-type", b"[[patterns.enabled]]\nname='x'\n"),
    ):
        yield dict(id=f"parser-{name}", command="install", global_bytes=content)


def fixture(root, case):
    for part in ("home", "project", "config/symbrain", "source", "data", "cache"):
        (root / part).mkdir(parents=True, exist_ok=True)
    (root / "config/symbrain/config.toml").write_bytes(case.get("global_bytes", b''))
    (root / "project/.symbrain.toml").write_bytes(case.get("project_bytes", b''))
    (root / "profile.toml").write_bytes(PROFILE)


def snapshot(root):
    result = {}
    for path in sorted(root.rglob("*")):
        relative = os.fsencode(path.relative_to(root)).hex()
        if path.is_symlink():
            result[relative] = dict(kind="symlink", target=encoded(os.fsencode(os.readlink(path))))
        else:
            result[relative] = dict(kind="directory" if path.is_dir() else "file",
                                    sha256=sha(path) if path.is_file() else None,
                                    mode=oct(path.stat().st_mode & 0o777))
    return result


def argv(root, case):
    return {
        "install": ["install", "--harness", "claude", "--dry-run"],
        "mcp": ["mcp", "--profile-file", str(root / "profile.toml")],
        "setup": ["setup", "--json"],
        "setup-fix": ["setup", "--fix", "--json"],
        "doctor-fix": ["doctor", "--fix"],
        "source": ["setup", "--from-source", str(root / "source"), "--json"],
        "config-get": ["config", "get", "audit.enabled"],
    }[case["command"]] + case.get("extra_args", [])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", help="Run one named case without changing its comparison")
    parser.add_argument("--control-native", type=Path,
                        help="Actual candidate delegated by the selected owned fault executable")
    parser.add_argument("--control-mode", choices=("premature-http", "premature-fs", "wrong-exit", "wrong-field-error"))
    parser.add_argument("--allow-fallback", action="store_true",
                        help="Preparatory route census only; cannot count as native acceptance")
    args = parser.parse_args()
    if bool(args.control_native) != bool(args.control_mode):
        parser.error("control-native and control-mode must be specified together")
    source_root = args.source_root.resolve()
    actual_head = subprocess.check_output(["git", "-C", str(source_root), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source_root), "status", "--porcelain"], text=True)
    if actual_head != args.source_head or dirty:
        raise SystemExit("process evidence requires the specified clean immutable source")
    source_files = [source_root / "Cargo.toml", source_root / "Cargo.lock"]
    source_files += sorted((source_root / "rust").rglob("*.rs"))
    source_files += sorted((source_root / "rust").rglob("Cargo.toml"))
    source_hashes = {str(path.relative_to(source_root)): sha(path) for path in source_files}
    requests, lock = [], threading.Lock()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            with lock:
                requests.append(self.path)
            self.send_response(503)
            self.end_headers()
            self.wfile.write(b"owned config admission refusal")

        def log_message(self, *_args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever)
    thread.start()
    base = {key: value for key, value in os.environ.items()
            if not key.startswith(("SYMBRAIN_", "SYMMEMORY_", "XDG_"))
            and key not in ("HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "PWD")}
    rows = []
    args.out.parent.mkdir(parents=True, exist_ok=True)

    def persist():
        args.out.write_text(json.dumps(dict(
            source_head=args.source_head, actual_os=sys.platform,
            candidate_source_sha256=source_hashes,
            runner_sha256=sha(__file__), go_sha256=sha(args.go), native_sha256=sha(args.native),
            delegated_native_sha256=sha(args.control_native) if args.control_native else None,
            actual_control_mode=args.control_mode,
            native_acceptance_mode=not args.allow_fallback, observations=rows,
            equal=sum(row["equal"] for row in rows), total=len(rows),
        ), indent=2) + "\n")

    try:
        with tempfile.TemporaryDirectory(prefix="brain-config13-gate-") as temporary:
            owned = Path(temporary)
            for case in cases():
                if args.case and args.case != case["id"]:
                    continue
                root = owned / case["id"]
                environment = dict(base, HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                    XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
                    XDG_CACHE_HOME=str(root / "cache"), PATH="",
                    SYMBRAIN_RELEASE_BASE_URL=f"http://127.0.0.1:{server.server_port}")
                environment.update(case.get("env", {}))
                observations = {}
                for name, binary in (("go", args.go), ("native", args.native)):
                    fixture(root, case)
                    before = snapshot(root)
                    with lock:
                        requests.clear()
                    env = dict(environment)
                    if name == "native":
                        env["SYMBRAIN_GO_BINARY"] = str(args.go if args.allow_fallback else owned / "missing-go")
                        if args.control_native:
                            env.update(CONFIG13_CONTROL_NATIVE=str(args.control_native.resolve()),
                                       CONFIG13_CONTROL_MODE=args.control_mode, CONFIG13_CONTROL_ROOT=str(root))
                    process = subprocess.run([str(binary.resolve()), *argv(root, case)], cwd=root / "project",
                                             env=env, input=b'', capture_output=True, timeout=20)
                    with lock:
                        release_requests = list(requests)
                    observations[name] = dict(exit=process.returncode,
                        stdout_b64=encoded(process.stdout), stderr_b64=encoded(process.stderr),
                        before=before, after=snapshot(root), owned_release_requests=release_requests)
                    shutil.rmtree(root)
                rows.append(dict(id=case["id"], command=case["command"],
                    global_b64=encoded(case.get("global_bytes", b'')),
                    project_b64=encoded(case.get("project_bytes", b'')),
                    extra_env=case.get("env", {}), **observations,
                    equal=observations["go"] == observations["native"]))
                persist()
    finally:
        server.shutdown()
        server.server_close()
        thread.join()
    print(json.dumps(dict(total=len(rows), equal=sum(row["equal"] for row in rows))))
    return 0 if rows and all(row["equal"] for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
