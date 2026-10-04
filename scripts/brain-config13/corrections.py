#!/usr/bin/env python3
"""Prepared source-bound full-byte pairs; no syntax/typing projection or Go edits.

Build/allocation and independent runtime review are still mandatory. This runner
has not executed product binaries at the source-only correction checkpoint.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

from correction_cases import consumer_cases, loader_cases
from process import PROFILE, encoded, fixture, sha, snapshot


def configuration(binary):
    # JSON string syntax is a subset of TOML basic-string syntax for these paths.
    return ('servers.vault.binary_path=' + json.dumps(str(binary), ensure_ascii=False)
            + '\n').encode()


def prepare(root, case, environment, vault_fixture):
    fixture(root, case)
    cwd = root / "project"
    if "profile_bytes" in case:
        environment["SYMBRAIN_DEFAULT_PROFILE"] = os.fsdecode(case["profile_bytes"])
    if case.get("existing"):
        # Canonical pinned-Go layouts containing the same literal U+FFFD value.
        if case["harness"] == "codex":
            path = root / "home/.codex/config.toml"
            content = ('model = "owned�"\n\n[mcp_servers]\n'
                       '  [mcp_servers.other]\n'
                       '    args = ["mcp", "--profile", "owned�"]\n'
                       '    command = "other"\n').encode()
        else:
            path = root / "home/.claude.json"
            content = (json.dumps(dict(mcpServers=dict(other=dict(
                args=["mcp", "--profile", "owned�"], command="other"))),
                ensure_ascii=False, indent=2) + '\n').encode()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    if case.get("command") != "vault" and not case.get("fatal"):
        return cwd
    if not vault_fixture:
        raise RuntimeError("Vault/fatal cases require the owned child fixture executable")
    suffix = ".exe" if os.name == "nt" else ""
    binary_dir = root / "binaries"
    binary_dir.mkdir()
    targets = {}
    for label in ("lexical-vault", "physical-vault", "fallback-vault"):
        path = binary_dir / (label + suffix)
        shutil.copyfile(vault_fixture, path)
        path.chmod(0o700)
        targets[label] = path
    managed = root / ("home/.symaira/bin/symvault" + suffix)
    managed.parent.mkdir(parents=True)
    shutil.copyfile(targets["fallback-vault"], managed)
    managed.chmod(0o700)
    environment.update(CONFIG13_VAULT_CAPTURE=str(root / "child-capture.json"))
    if case.get("fatal"):
        # If a broken consumer admits an invalid config, an enabled private
        # Vault fixture leaves a durable child observation, without a service.
        (root / "profile.toml").write_bytes(PROFILE.replace(
            b"[servers.vault]\nenabled=false", b"[servers.vault]\nenabled=true"))
        return cwd
    kind = case["pwd_case"]
    (cwd / ".symbrain.toml").write_bytes(configuration(targets["lexical-vault"]))
    if kind in ("relative-ignored", "mismatch-ignored"):
        environment["PWD"] = "../project" if kind == "relative-ignored" else str(root / "home")
        return cwd
    for directory in ("owner/deep", "owner/project"):
        (root / directory).mkdir(parents=True)
    cwd = root / "owner/project"
    (cwd / ".symbrain.toml").write_bytes(configuration(targets["physical-vault"]))
    if kind == "lexical-type":
        (root / "project/.symbrain.toml").write_bytes(b'audit.enabled="bad"\n')
    elif kind == "physical-type":
        (cwd / ".symbrain.toml").write_bytes(b'audit.enabled="bad"\n')
    link = root / (os.fsdecode(b"link\xff") if case.get("raw_link") else "link")
    link.symlink_to(root / "owner/deep", target_is_directory=True)
    environment["PWD"] = str(link) + "/../project"
    return cwd


def command(root, case, kind):
    if kind == "values":
        return []
    if case["command"] == "install":
        return ["install", "--harness", case["harness"]] + (["--dry-run"] if case["dry"] else [])
    if case["command"] == "mcp":
        return ["mcp", "--profile-file", str(root / "profile.toml")]
    return ["vault", "config13-owned-probe", "arg with space"]


def input_record(case):
    return {key: {"bytes_b64": encoded(value)} if isinstance(value, bytes) else value
            for key, value in case.items()}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for argument in ("go", "native", "source-root", "go-contract-root", "out"):
        parser.add_argument("--" + argument, type=Path, required=True)
    parser.add_argument("--source-head", required=True)
    parser.add_argument("--kind", choices=("consumers", "values"), required=True)
    parser.add_argument("--vault-fixture", type=Path)
    parser.add_argument("--case")
    parser.add_argument("--control-native", type=Path)
    parser.add_argument("--control-mode", choices=("wrong-owner", "repair-codex", "reject-prefix"))
    args = parser.parse_args()
    if bool(args.control_native) != bool(args.control_mode):
        parser.error("control-native and control-mode must be paired")
    source = args.source_root.resolve()
    head = subprocess.check_output(["git", "-C", str(source), "rev-parse", "HEAD"], text=True).strip()
    dirty = subprocess.check_output(["git", "-C", str(source), "status", "--porcelain"], text=True)
    if head != args.source_head or dirty:
        raise SystemExit("requires the specified clean immutable source")
    files = [source / "Cargo.toml", source / "Cargo.lock"]
    for directory, pattern in (("rust", "*.rs"), ("rust", "Cargo.toml"),
                               ("scripts/brain-config13", "*.py"), ("scripts/brain-config13", "*.go")):
        files.extend(sorted((source / directory).rglob(pattern)))
    contract = args.go_contract_root.resolve()
    go_files = [contract / "go.mod", contract / "go.sum"]
    for directory in ("cmd", "internal"):
        go_files.extend(sorted((contract / directory).rglob("*.go")))
    if not go_files[0].is_file() or not (contract / "internal/config/config.go").is_file():
        raise SystemExit("requires the retained full frozen Go contract tree")
    selected = [case for case in (consumer_cases() if args.kind == "consumers" else loader_cases())
                if not args.case or args.case == case["id"]]
    if not selected:
        raise SystemExit("requested case is absent")
    base = {key: value for key, value in os.environ.items()
            if not key.startswith(("SYMBRAIN_", "SYMMEMORY_", "XDG_", "CONFIG13_"))
            and key not in ("HOME", "USERPROFILE", "HOMEDRIVE", "HOMEPATH", "PWD")}
    rows, pending = [], None
    metadata = dict(source_head=head, actual_os=sys.platform, kind=args.kind,
                    candidate_source_sha256={str(p.relative_to(source)): sha(p) for p in files},
                    frozen_go_source_sha256={str(p.relative_to(contract)): sha(p) for p in go_files},
                    go_sha256=sha(args.go), native_sha256=sha(args.native),
                    vault_fixture_sha256=sha(args.vault_fixture) if args.vault_fixture else None,
                    delegated_native_sha256=sha(args.control_native) if args.control_native else None,
                    actual_control_mode=args.control_mode)
    args.out.parent.mkdir(parents=True, exist_ok=True)

    def persist(status):
        args.out.write_text(json.dumps(dict(metadata, status=status, pending=pending,
            observations=rows, total=len(rows), equal=sum(row["equal"] for row in rows)),
            indent=2, ensure_ascii=True) + '\n', encoding="utf-8")

    persist("started")
    with tempfile.TemporaryDirectory(prefix="brain13-corrections-owned-") as temporary:
        owned = Path(temporary)
        for case in selected:
            root = owned / case["id"]
            observations = {}
            for who, binary in (("go", args.go), ("native", args.native)):
                environment = dict(base, HOME=str(root / "home"), USERPROFILE=str(root / "home"),
                    XDG_CONFIG_HOME=str(root / "config"), XDG_DATA_HOME=str(root / "data"),
                    XDG_CACHE_HOME=str(root / "cache"), PATH="", CONFIG13_CONTROL_ROOT=str(root))
                if who == "native":
                    environment["SYMBRAIN_GO_BINARY"] = str(owned / "missing-go-fallback")
                    if args.control_native:
                        environment.update(CONFIG13_CONTROL_NATIVE=str(args.control_native.resolve()),
                                           CONFIG13_CONTROL_MODE=args.control_mode)
                cwd = prepare(root, case, environment, args.vault_fixture)
                before = snapshot(root)
                pending = dict(id=case["id"], who=who, input=input_record(case), before=before,
                               cwd_b64=encoded(os.fsencode(cwd)))
                persist("running")
                try:
                    process = subprocess.run([str(binary.resolve()), *command(root, case, args.kind)],
                        cwd=cwd, env=environment, input=b'', capture_output=True, timeout=20)
                except subprocess.TimeoutExpired as error:
                    pending.update(timeout=True, stdout_b64=encoded(error.stdout or b''),
                                   stderr_b64=encoded(error.stderr or b''), after=snapshot(root))
                    persist("failed-timeout")
                    raise
                after = snapshot(root)
                observations[who] = dict(exit=process.returncode, stdout_b64=encoded(process.stdout),
                    stderr_b64=encoded(process.stderr), before=before, after=after)
                if case.get("fatal"):
                    observations[who]["fatal_contract"] = process.returncode == 2 and before == after
                if case.get("positive"):
                    observations[who]["positive_contract"] = process.returncode == 0
                pending.update(observation=observations[who])
                persist("observed")
                shutil.rmtree(root)
            equal = observations["go"] == observations["native"]
            contracts = all(observation.get("fatal_contract", True)
                            and observation.get("positive_contract", True)
                            for observation in observations.values())
            rows.append(dict(id=case["id"], input=input_record(case), **observations,
                             equal=equal and contracts))
            pending = None
            persist("running")
    persist("finished")
    print(json.dumps(dict(total=len(rows), equal=sum(row["equal"] for row in rows))))
    return 0 if all(row["equal"] for row in rows) else 1


if __name__ == "__main__":
    raise SystemExit(main())
