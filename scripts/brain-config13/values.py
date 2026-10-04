#!/usr/bin/env python3
"""Strict actual Go/native loader-value pairs; no stores or CLI fallback.

Both executables must be built from the immutable source receipts after allocation.
The Go fixture imports a byte-identical owned copy of frozen internal/config.
This prepared corpus has not been executed at the precompile checkpoint.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

from process import FIELDS, encoded, fixture, sha, snapshot

VALID = [
    ("default_profile", '"owned"', "owned"),
    ("audit.enabled", "false", "false"),
    ("audit.verbose", "true", "true"),
    ("gateway.identity_injection", "false", "false"),
    ("updatecheck.enabled", "false", "false"),
    ("servers.vault.binary_path", '"owned-vault"', "owned-vault"),
    ("servers.operate.binary_path", '"owned-operate"', "owned-operate"),
    ("servers.scope.binary_path", '"owned-scope"', "owned-scope"),
    ("patterns.enabled", "false", "false"),
    ("patterns.promotion_threshold", "5", "5"),
    ("modules.browse", "true", "true"),
    ("modules.operate", "true", "true"),
    ("modules.scope", "true", "true"),
]


def cases():
    yield dict(id="defaults", absent_global=True, absent_project=True)
    yield dict(id="empty-documents")
    yield dict(id="home-required-before-absolute-xdg", env_unset=["HOME", "USERPROFILE"])
    for field, document in FIELDS:
        for stage in ("global", "project"):
            yield dict(id=f"wrong-{stage}-{field}", **{stage + "_bytes": document})
    for field, literal, environment in VALID:
        content = f"{field}={literal}\n".encode()
        key = "SYMBRAIN_" + field.replace(".", "_").upper()
        yield dict(id=f"valid-global-{field}", global_bytes=content)
        yield dict(id=f"valid-project-{field}", project_bytes=content)
        yield dict(id=f"valid-env-{field}", env={key: environment})
        yield dict(id=f"empty-env-{field}", global_bytes=content, env={key: ""})
    for name, global_bytes, project_bytes, environment in (
        ("global-priority", b'audit.enabled="globalbad"\n', b'audit.verbose="projectbad"\n', {"SYMBRAIN_AUDIT_ENABLED": "envbad"}),
        ("project-priority", b'', b'audit.verbose="projectbad"\n', {"SYMBRAIN_AUDIT_ENABLED": "envbad"}),
        ("file-declaration-priority", b'modules.scope="bad"\naudit.enabled="bad"\n', b'', {}),
        ("env-declaration-priority", b'', b'', {"SYMBRAIN_MODULES_SCOPE": "bad", "SYMBRAIN_AUDIT_ENABLED": "bad"}),
        ("plain-zero-project", b'default_profile="global"\naudit.verbose=true\nmodules.browse=true\npatterns.promotion_threshold=5\n', b'default_profile=""\naudit.verbose=false\nmodules.browse=false\npatterns.promotion_threshold=0\n', {}),
        ("pointer-false-project", b'audit.enabled=true\ngateway.identity_injection=true\npatterns.enabled=true\nupdatecheck.enabled=true\n', b'audit.enabled=false\ngateway.identity_injection=false\npatterns.enabled=false\nupdatecheck.enabled=false\n', {}),
        ("ignored-struct-scalars", b'audit="ignored"\nmodules=[]\nservers=false\npatterns=7\n', b'', {}),
        ("ignored-unknown", b'unknown={audit="bad"}\n', b'', {}),
        ("struct-scalar-project", b'audit.enabled=false\nmodules.browse=true\n', b'audit="ignored"\nmodules=[]\n', {}),
    ):
        yield dict(id=name, global_bytes=global_bytes, project_bytes=project_bytes, env=environment)
    for value in ("1", "t", "T", "true", "TRUE", "True", "0", "f", "F", "false", "FALSE", "False", "yes", " true", "true "):
        yield dict(id="bool-alias-" + value.encode().hex(), env={"SYMBRAIN_AUDIT_ENABLED": value})
    for value in ("0", "-2", "3", "+4", "9223372036854775807", "-9223372036854775808", "9223372036854775808", "-9223372036854775809", "18446744073709551616x", "99999999999999999999999", "4_0", " 4", "4 ", "+", "0xff"):
        yield dict(id="int-" + value.encode().hex(), env={"SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD": value})
    for value in ("4.9", "-4.9", "0.0", "inf", "-inf", "nan", "9.223372036854776e18"):
        yield dict(id="float-" + value.encode().hex(), global_bytes=f"patterns.promotion_threshold={value}\n".encode())
    for name, content in (
        ("original-bad-table", b"[bad config"),
        ("comments-crlf", b'# comment\r\ndefault_profile="owned"\r\n'),
        ("quoted-key", b'"audit"."enabled"=false\n'),
        ("inline-tables", b'audit={enabled=false,verbose=true}\n'),
        ("multiline-basic", b'default_profile="""owned"""\n'),
        ("literal", b"default_profile='owned'\n"),
        ("utf8", 'default_profile="owned\u00e9\u2028"\n'.encode()),
        ("numeric-underscore", b'patterns.promotion_threshold=1_0\n'),
        ("hexadecimal", b'patterns.promotion_threshold=0x10\n'),
        ("duplicate-key", b'audit.enabled=true\naudit.enabled=false\n'),
        ("duplicate-table", b'[audit]\nenabled=true\n[audit]\nverbose=true\n'),
        ("bad-array", b'invalid=[unterminated\n'),
        ("invalid-utf8", b'#\xff\n'),
        ("grammar-before-utf8", b'invalid=[unterminated\n#\xff\n'),
        ("date-type", b'default_profile=1979-05-27T07:32:00Z\n'),
        ("array-type", b'audit.enabled=[false]\n'),
        ("array-table-type", b'[[audit.enabled]]\nname="bad"\n'),
    ):
        yield dict(id="parser-" + name, global_bytes=content)
    if os.name == "posix":
        yield dict(id="raw-default-env", env={"SYMBRAIN_DEFAULT_PROFILE": os.fsdecode(b"owned\xff\xe2\x82")})
        yield dict(id="raw-bool-env", env={"SYMBRAIN_AUDIT_ENABLED": os.fsdecode(b"true\xff")})
        yield dict(id="raw-int-env", env={"SYMBRAIN_PATTERNS_PROMOTION_THRESHOLD": os.fsdecode(b"3\xff")})
        for kind in ("lexical-symlink", "raw-symlink", "relative-ignored", "mismatch-ignored", "repeated-separators"):
            yield dict(id="pwd-" + kind, pwd_case=kind)
    if os.name == "nt":
        yield dict(id="windows-pwd-ignored", pwd_case="mismatch-ignored")
        yield dict(id="windows-userprofile-only", env_unset=["USERPROFILE"],
                   env={"HOMEDRIVE": "C:", "HOMEPATH": "\\owned-fallback"})
        for units in ("\ud800", "\udc00", "\ud800\udc00", "\ufffd"):
            label = "-".join(f"{ord(unit):04x}" for unit in units)
            yield dict(id="windows-wide-" + label, wide_owner=units)
            for field in ("DEFAULT_PROFILE", "SERVERS_VAULT_BINARY_PATH", "SERVERS_OPERATE_BINARY_PATH",
                          "SERVERS_SCOPE_BINARY_PATH", "AUDIT_ENABLED", "PATTERNS_PROMOTION_THRESHOLD"):
                yield dict(id="windows-env-" + field.lower() + "-" + label,
                           env={"SYMBRAIN_" + field: "owned-" + units})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--native", type=Path, required=True)
    parser.add_argument("--source-root", type=Path, required=True)
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--case", help="Run one named case without changing its comparison")
    parser.add_argument("--control-native", type=Path)
    parser.add_argument("--control-mode", choices=("premature-fs", "wrong-exit", "wrong-field-error", "wrong-value"))
    args = parser.parse_args()
    if bool(args.control_native) != bool(args.control_mode):
        parser.error("control-native and control-mode must be specified together")
    source = args.source_root.resolve()
    head = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    if head != args.source_head or dirty:
        raise SystemExit("value gate requires the specified clean immutable source")
    rows = []
    args.out.parent.mkdir(parents=True, exist_ok=True)
    base = {k: v for k, v in os.environ.items() if not k.startswith(("SYMBRAIN_", "SYMMEMORY_", "XDG_"))
            and k not in ("HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "PWD")}
    with tempfile.TemporaryDirectory(prefix="brain-config13-values-") as temporary:
        for case in cases():
            if args.case and args.case != case["id"]:
                continue
            root = Path(temporary) / case["id"]
            observations = {}
            for who, binary in (("go", args.go), ("native", args.native)):
                fixture(root, case)
                for stage in ("global", "project"):
                    if case.get("absent_" + stage):
                        (root / ("config/symbrain/config.toml" if stage == "global" else "project/.symbrain.toml")).unlink()
                environment = dict(base, HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                                   XDG_CONFIG_HOME=str(root / "config"), PATH="")
                environment.update(case.get("env", {}))
                for variable in case.get("env_unset", []):
                    environment.pop(variable, None)
                if "wide_owner" in case:
                    config_dir = root / ("wide-" + case["wide_owner"])
                    (config_dir / "symbrain").mkdir(parents=True)
                    (config_dir / "symbrain/config.toml").write_bytes(b'default_profile="wide-owner"\n')
                    environment["XDG_CONFIG_HOME"] = str(config_dir)
                cwd = root / "project"
                if case.get("pwd_case"):
                    kind = case["pwd_case"]
                    if kind in ("lexical-symlink", "raw-symlink"):
                        for directory in ("owner/deep", "owner/project"):
                            (root / directory).mkdir(parents=True)
                        (root / "owner/project/.symbrain.toml").write_bytes(b'default_profile="physical-owner"\n')
                        (root / "project/.symbrain.toml").write_bytes(b'default_profile="lexical-owner"\n')
                        link = root / (os.fsdecode(b"link\xff") if kind == "raw-symlink" else "link")
                        link.symlink_to(root / "owner/deep", target_is_directory=True)
                        cwd = root / "owner/project"
                        environment["PWD"] = str(link) + "/../project"
                    elif kind == "relative-ignored":
                        environment["PWD"] = "../project"
                    elif kind == "mismatch-ignored":
                        environment["PWD"] = str(root / "home")
                    else:
                        environment["PWD"] = str(root / "project") + "//./"
                if who == "native" and args.control_native:
                    environment.update(CONFIG13_CONTROL_NATIVE=str(args.control_native.resolve()),
                                       CONFIG13_CONTROL_MODE=args.control_mode, CONFIG13_CONTROL_ROOT=str(root))
                before = snapshot(root)
                process = subprocess.run([str(binary.resolve())], cwd=cwd, env=environment,
                                         input=b'', capture_output=True, timeout=10)
                observations[who] = dict(exit=process.returncode, stdout_b64=encoded(process.stdout),
                                        stderr_b64=encoded(process.stderr), before=before, after=snapshot(root))
                shutil.rmtree(root)
            rows.append(dict(id=case["id"], global_b64=encoded(case.get("global_bytes", b'')),
                             project_b64=encoded(case.get("project_bytes", b'')),
                             extra_env={k: encoded(os.fsencode(v)) for k, v in case.get("env", {}).items()},
                             **observations, equal=observations["go"] == observations["native"]))
            args.out.write_text(json.dumps(dict(source_head=head, actual_os=os.name,
                runner_sha256=sha(__file__), go_sha256=sha(args.go), native_sha256=sha(args.native),
                delegated_native_sha256=sha(args.control_native) if args.control_native else None,
                actual_control_mode=args.control_mode,
                observations=rows, total=len(rows), equal=sum(r["equal"] for r in rows)), indent=2) + "\n")
    print(json.dumps(dict(total=len(rows), equal=sum(r["equal"] for r in rows))))
    return 0 if rows and all(r["equal"] for r in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
