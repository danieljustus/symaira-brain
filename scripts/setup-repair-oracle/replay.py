#!/usr/bin/env python3
"""Replay release repair against the immutable full Go CLI, with no Rust fallback."""
from __future__ import annotations

import base64
from dataclasses import replace
import hashlib
import importlib.util
import json
import os
import platform
import shutil
import stat
import time
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))
spec = importlib.util.spec_from_file_location("repair_legacy", ROOT / "scripts/rust-differential.py")
legacy = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = legacy
spec.loader.exec_module(legacy)


def source_fixture(payload: bytes):
    def setup(root, env):
        legacy.setup_mismatched_managed_binaries(root, env)
        path = Path(env["HOME"]) / ".symaira/bin/symdesk.provenance.json"
        path.write_bytes(payload)
    return setup


def managed_boundary(lexical=None, raw=False, fault=None):
    def setup(root, env):
        home=env["HOME"]
        if raw:home=str(root/os.fsdecode(b"home\xff\xe2\x82"))
        if lexical=="dot":home+="/./"
        elif lexical=="parent":home+="/../"+Path(home).name
        elif lexical=="slash":home+="//"
        elif lexical=="symlink-parent":
            (root/"owner/nested").mkdir(parents=True)
            (root/"link").symlink_to("owner/nested",target_is_directory=True)
            home=str(root/"link")+"/../home"
        env["HOME"]=home
        if os.name=="nt":env["USERPROFILE"]=home
        legacy.setup_correct_managed_binaries(root,env)
        if fault:
            binary_dir=Path(home)/".symaira/bin"
            blocked=binary_dir if fault=="bin" else binary_dir.parent
            shutil.rmtree(blocked);blocked.write_bytes(b"owned obstruction")
    return setup


def escaping_fixture(raw=False, lexical=False, fault=None):
    def setup(root, env):
        # Preserve the original review bytes on Unix. Windows filenames cannot
        # contain <>; its valid owner still exercises &, U+2028 and U+2029.
        name = "home&" + ("<>" if os.name != "nt" else "") + "\u2028\u2029"
        if raw:
            name = os.fsdecode(b"home\xff\xef\xbf\xbd\xe2\x82&<>\xe2\x80\xa8\xe2\x80\xa9")
        home = root / name
        home.mkdir()
        env["HOME"] = str(home) + ("/../" + name + "/./" if lexical else "")
        if os.name == "nt":
            env["USERPROFILE"] = env["HOME"]
        legacy.setup_release_fixture(root, env)
        if fault:
            parent = home / ".symaira"
            if fault == "bin":
                parent.mkdir()
                (parent / "bin").write_bytes(b"owned leaf obstruction")
            else:
                parent.write_bytes(b"owned ancestor obstruction")
    return setup


def cases():
    result = [case for case in legacy.CASES if case.name.startswith("setup_")]
    payloads = [
        b'{"source":"brain-source"}',
        b'{"SOURCE":"brain-source"}',
        '{"ſOURCE":"brain-source"}'.encode(),
        b'{"source":"brain-source","source":null}',
        b'{"source":"release","SOURCE":"brain-source"}',
        b'{"source":"brain-source","source":"release"}',
        b'{"source":"brain-source","version":null,"built_at":null}',
        b'{"source":"brain-source","built_at":"2026-10-03T12:00:00Z"}',
        b'{"source":"brain-source","built_at":"2026-10-03T12:00:00.123456789+02:00"}',
        b'{"source":"brain-source","built_at":"invalid"}',
        b'{"source":"brain-source","built_at":"2026-10-03t12:00:00z"}',
        b'{"source":"brain-source","built_at":"2026-10-03T12:00:60Z"}',
        b'{"source":"brain-source","version":42}',
        b'{"source":"brain-source","binary":false}',
        b'{"source":"brain-source","version":42,"version":"ok"}',
        b'{"source":"brain-source","unknown":{"x":1e1000}}',
        b'{"source":"brain-source","version":"\\ud800"}',
        b'{"source":"brain-source","version":"bad\xff\xfe"}',
        b'{"source":"brain-source","\\ud800":"ignored"}',
        b'{"source":null}', b'null', b'[]', b'{bad',
    ]
    for timestamp in (
        "0000-01-01T00:00:00Z", "2024-02-29T00:00:00Z", "2026-02-29T00:00:00Z",
        "2026-10-03T2:00:00Z", "2026-10-03T12:00:00,123Z",
        "2026-10-03T12:00:00.123456789123456789Z", "2026-10-03T12:00:00+24:00",
        "2026-10-03T12:00:00+01:60", "2026-10-03T12:00:00+25:00",
        "2026-10-03T12:00:00+01:61", "2026-10-03T24:00:00Z",
        "2026-10-03T12:0:00Z", "2026-10-03T12:00:0Z", "2026-10-03T12:00:00.Z",
    ):
        payloads.append(json.dumps({"source": "brain-source", "built_at": timestamp}).encode())
    payloads.append(b'{"source":"brain-source","built_at":"2026-10-03T12:00:00\\u005a"}')
    payloads.append(b'{"source":"brain-source","unknown":' + b'[' * 200 + b'0' + b']' * 200 + b'}')
    for index, payload in enumerate(payloads):
        for force in (False, True):
            result.append(legacy.Case(
                f"repair-sidecar-{index}-force-{force}",
                ("setup", "--fix", "--allow-unsigned", "--json",
                 f"--force-release={'true' if force else 'false'}"),
                setup=source_fixture(payload), mutating=True,
            ))
    for name in ("json", "fix", "allow-unsigned", "force-release"):
        for value in ("1", "t", "T", "TRUE", "True", "true",
                      "0", "f", "F", "FALSE", "False", "false", "", "yes"):
            result.append(legacy.Case(
                f"repair-bool-{name}-{value}",
                ("setup", "--fix", "--allow-unsigned", "--json", f"--{name}={value}"),
                setup=legacy.setup_correct_managed_binaries, mutating=True,
            ))
    result.append(legacy.Case(
        "repair-source-human", ("setup", "--fix"),
        setup=source_fixture(b'{"source":"brain-source"}'), mutating=True,
    ))
    for index, config in enumerate((
        '[modules]\nbrowse = true', '[modules]\nbrowse = false',
        '[modules]\nbrowse = "TRUE"', '[modules]\nbrowse = "false"',
        '[modules]\nbrowse = 0', '[modules]\nbrowse = ""', 'modules = 1',
    )):
        def configured(root, env, text=config):
            legacy.setup_correct_managed_binaries(root, env)
            path = Path(env["XDG_CONFIG_HOME"]) / 'symbrain/config.toml'
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text)
        result.append(legacy.Case(f"repair-module-config-{index}",
            ("setup", "--fix", "--allow-unsigned", "--json"), setup=configured, mutating=True))
    for json_out in (False,True):
        args=("setup","--fix","--allow-unsigned")+(("--json",) if json_out else ())
        for lexical in ("dot","parent","slash"):
            result.append(legacy.Case(f"managed-home-{lexical}-{json_out}",args,
                          setup=managed_boundary(lexical=lexical),mutating=True))
        for fault in ("bin","parent"):
            result.append(legacy.Case(f"managed-file-{fault}-{json_out}",args,
                          setup=managed_boundary(fault=fault),mutating=True))
            if os.name!="nt":
                result.append(legacy.Case(f"raw-managed-file-{fault}-{json_out}",args,
                              setup=managed_boundary(raw=True,fault=fault),mutating=True))
        if os.name!="nt":
            for lexical in (None,"symlink-parent"):
                result.append(legacy.Case(f"managed-owner-{lexical}-{json_out}",args,
                              setup=managed_boundary(raw=lexical is None,lexical=lexical),mutating=True))
    for raw in ((False, True) if os.name != "nt" else (False,)):
        for fix in (False, True):
            for json_out in (False, True):
                args = ("setup",) + (("--fix",) if fix else ()) + ("--allow-unsigned",) + (("--json",) if json_out else ())
                for fault, lexical in ((None, False), (None, True), ("bin", True), ("parent", True)):
                    result.append(legacy.Case(f"json-escape-raw-{raw}-fix-{fix}-fault-{fault}-lexical-{lexical}-json-{json_out}", args,
                        setup=escaping_fixture(raw, lexical, fault), mutating=True))
    return result


def main():
    if len(sys.argv) != 4:
        raise SystemExit("usage: replay.py GO_BINARY RUST_BINARY REPORT_JSON")
    go, rust, report = sys.argv[1:]
    selected = cases()
    assert len({case.name for case in selected}) == len(selected)
    legacy.CASES = tuple(replace(case, env_overrides={
        **(case.env_overrides or {}), "SYMBRAIN_GO_BINARY": str(ROOT / "absent-go-fallback")
    }) for case in selected)
    observations = []
    original_sidecars = {}
    started = time.time()
    def filesystem(root):
        result = {}
        for path in sorted(root.rglob("*")):
            relative = str(path.relative_to(root))
            mode = stat.S_IMODE(path.lstat().st_mode)
            if path.is_symlink():
                result[relative] = ("link", mode, str(path.readlink()))
            elif path.is_dir():
                result[relative] = ("dir", mode)
            else:
                content = legacy.normalize_fixture_root(path.read_bytes(), root)
                if relative.endswith(".provenance.json") and content != original_sidecars.get(str(path)):
                    try:
                        value = json.loads(content)
                    except (ValueError, UnicodeError):
                        value = None
                    if isinstance(value, dict) and value.get("source") == "release":
                        timestamp = value["built_at"]
                        parsed = legacy.datetime.fromisoformat(timestamp.replace("Z", "+00:00"))
                        assert timestamp.endswith("Z") and started - 1 <= parsed.timestamp() <= time.time() + 1
                        content = content.replace(timestamp.encode(), b"<install-timestamp>")
                result[relative] = ("file", mode, content)
        return result
    legacy.get_fs_manifest = filesystem
    def summarized_files(root):
        manifest = filesystem(root)
        return {name: {"type": entry[0], "mode": entry[1], **(
            {"sha256": hashlib.sha256(entry[2]).hexdigest(), "size": len(entry[2]),
             **({"content_base64": base64.b64encode(entry[2]).decode()} if len(entry[2]) < 4096 else {})}
            if entry[0] == "file" else {"target": entry[2]} if entry[0] == "link" else {})}
            for name, entry in manifest.items()}
    actual_run = legacy.run
    def recorded_run(binary, args, env, stdin=None, pty=False):
        fixture_root = Path(env["PROJECT"]).parent
        if Path(binary).resolve() in (Path(go).resolve(), Path(rust).resolve()):
            original_sidecars.update({str(path): legacy.normalize_fixture_root(path.read_bytes(), fixture_root)
                                     for path in fixture_root.rglob("*.provenance.json")})
            before = summarized_files(fixture_root)
        completed = actual_run(binary, args, env, stdin, pty)
        if Path(binary).resolve() not in (Path(go).resolve(), Path(rust).resolve()):
            return completed
        observations.append({
            "binary": str(binary), "args": args, "exit": completed.returncode,
            "stdout_base64": base64.b64encode(completed.stdout).decode(),
            "stderr_base64": base64.b64encode(completed.stderr).decode(),
            "fixture_root": str(fixture_root),
            "filesystem_before": before, "filesystem_after": summarized_files(fixture_root),
        })
        return completed
    legacy.run = recorded_run
    sys.argv = [str(ROOT / "scripts/rust-differential.py"), go, rust]
    code = legacy.main()
    assert len(observations) == 2 * len(selected), "incomplete process replay"
    data = {
        "go_oracle_ref": "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c",
        "candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip(),
        "candidate_dirty": bool(subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT)),
        "runtime": platform.platform(), "total": len(selected), "exit": code,
        "matches": code == 0, "cases": [case.name for case in selected],
        "observations": observations,
        "go_binary_sha256": hashlib.sha256(Path(go).read_bytes()).hexdigest(),
        "rust_binary_sha256": hashlib.sha256(Path(rust).read_bytes()).hexdigest(),
        "comparison": "stdout/stderr/exit and full fixture filesystem/modes; only fixture roots, release scratch names and newly written release timestamps (validated within this run) normalized; pre-existing source sidecars remain exact",
        "fallback": "every Rust process has an absent SYMBRAIN_GO_BINARY; no Go fallback can produce matching successful repair",
    }
    names = subprocess.check_output(['git', 'ls-tree', '-r', '--name-only', data['go_oracle_ref'], '--',
        'cmd/symbrain', 'internal/managed', 'internal/config', 'internal/xdg', 'go.mod', 'go.sum'], cwd=ROOT, text=True).splitlines()
    data['go_source_sha256'] = {name: hashlib.sha256(subprocess.check_output([
        'git', 'show', f"{data['go_oracle_ref']}:{name}"], cwd=ROOT)).hexdigest()
        for name in names if name.endswith('.go') or name in ('go.mod', 'go.sum')}
    data['candidate_source_sha256'] = {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in (
        'rust/symbrain-cli/src/setup_cli.rs', 'rust/symbrain-cli/src/setup_args.rs',
        'rust/symbrain-cli/src/setup_config.rs', 'rust/symbrain-cli/src/vault_config.rs',
        'rust/symbrain-managed/src/provenance.rs', 'rust/symbrain-managed/src/provenance_json.rs',
        'rust/symbrain-managed/src/install.rs', 'rust/symbrain-managed/src/version_probe.rs',
        'rust/symbrain-cli/src/go_json_escape.rs', 'rust/symbrain-managed/src/lib.rs',
        'rust/symbrain-core/src/go_text.rs', 'rust/symbrain-core/src/json_string.rs',
        'scripts/setup-repair-oracle/replay.py', 'scripts/rust-differential.py')}
    Path(report).write_text(json.dumps(data, indent=2) + "\n")
    return code


if __name__ == "__main__":
    raise SystemExit(main())
