#!/usr/bin/env python3
"""Re-record / re-verify Go-generated oracle fixtures. REQUIRES A GO TOOLCHAIN.

This helper is deliberately NOT part of ``make rust-check``; it is reached only
through the ``record-*`` Makefile targets and will be deleted together with the
Go sources. It never touches operator state: every generator runs in a
disposable ``git worktree`` of HEAD with a scrubbed allow-list environment
(no HERMES_*, API keys, tokens or provider variables) and an isolated HOME for
tools that honour it.

  check   regenerate each Go-generator group in a scratch worktree and compare
          byte-for-byte with the committed fixtures (what the legacy
          ``*-oracle-check`` targets did); with --seal, stamp the result into
          the manifest as ``regeneration_verified``
  record  regenerate, copy the fresh bytes over the committed fixtures, re-seal
"""
from __future__ import annotations

import argparse
import datetime
import os
import platform
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import freeze  # noqa: E402

ROOT = freeze.ROOT
ENV_ALLOW = ("PATH", "HOME", "USER", "LANG", "LC_ALL", "TZ", "TMPDIR", "TMP", "TEMP", "SHELL",
             "CARGO_HOME", "RUSTUP_HOME", "PYTHONPYCACHEPREFIX")
ENV_ALLOW_PREFIX = ("GO", "CGO_", "SYMAIRA_EXTERNAL")


def scrubbed_env(extra: dict[str, str]) -> dict[str, str]:
    env = {k: v for k, v in os.environ.items() if k in ENV_ALLOW or k.startswith(ENV_ALLOW_PREFIX)}
    env.update(extra)
    return env


def host_goos() -> str:
    return {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}.get(platform.system(), platform.system().lower())


def go_toolchain_default() -> str:
    for line in (ROOT / "go.mod").read_text().splitlines():
        if line.startswith("go "):
            return "go" + line.split()[1]
    raise SystemExit("go.mod has no go directive")


def run(argv: list[str], cwd: Path, env: dict[str, str], stdout_path: Path | None = None) -> None:
    print("+", " ".join(argv), flush=True)
    stdout = stdout_path.open("wb") if stdout_path else None
    try:
        proc = subprocess.run(argv, cwd=cwd, env=env, stdout=stdout, stderr=subprocess.PIPE if stdout else None, check=False)
    finally:
        if stdout:
            stdout.close()
    if proc.returncode != 0:
        detail = (proc.stderr or b"").decode(errors="replace")[-2000:] if stdout else ""
        raise RuntimeError(f"{' '.join(argv)} exited {proc.returncode}\n{detail}")


def regenerate(group: dict, worktree: Path, scratch: Path, mode: str) -> str | None:
    """Run one generator in the scratch worktree.

    mode "write": run the generator so it rewrites its fixtures inside the worktree.
    mode "check": use the generator's own verification semantics (its `-check`
    flag, the Go test without its UPDATE switch, or a normalised stdout
    comparison for generators that embed random ids). Returns a drift message
    for the normalised-stdout comparison, otherwise None; failures raise.
    """
    record = group["record"]
    toolchain = record.get("toolchain", "go.mod")
    toolchain = go_toolchain_default() if toolchain == "go.mod" else toolchain
    extra_env = dict(record.get("env", {}))
    if mode == "check" and record.get("kind") == "go-test":
        extra_env = {k: v for k, v in extra_env.items() if not k.startswith("UPDATE_")}
    env = scrubbed_env({"GOTOOLCHAIN": toolchain, "CGO_ENABLED": "0", **extra_env})
    args = list(record["args"])
    flag = record.get("go_binary_flag")
    if flag:
        binary = scratch / "symbrain-go"
        run(["go", "-C", str(worktree), "build", "-ldflags", "-X main.version=dev", "-o", str(binary), "./cmd/symbrain"], worktree, env)
        args += [flag, str(binary)]
    if record.get("stdout_to_fixture"):
        target = worktree / group["fixtures"][0] if mode == "write" else scratch / "stdout.json"
        run(["go", "-C", str(worktree), *args], worktree, env, stdout_path=target)
        if mode == "check":
            import re
            fresh, committed = target.read_bytes().decode(), (ROOT / group["fixtures"][0]).read_bytes().decode()
            for pattern in record.get("normalize", []):
                fresh = re.sub(pattern, "<random>", fresh)
                committed = re.sub(pattern, "<random>", committed)
            if fresh != committed:
                return f"{group['fixtures'][0]}: differs from the committed bytes beyond {record.get('normalize')}"
        return None
    if mode == "check" and record.get("check_arg"):
        args += [record["check_arg"]]
    run(["go", "-C", str(worktree), *args], worktree, env)
    return None


def go_version(toolchain_env: dict[str, str]) -> str:
    out = subprocess.run(["go", "version"], env=toolchain_env, capture_output=True, text=True, check=True)
    return out.stdout.strip()


def groups_to_run(registry: dict, wanted: set[str] | None) -> tuple[list[dict], list[tuple[str, str]]]:
    selected, skipped = [], []
    for group in registry["groups"]:
        if wanted is not None and group["id"] not in wanted:
            continue
        kind = group["record"].get("kind")
        if kind not in ("go", "go-test"):
            skipped.append((group["id"], f"not auto-regenerable here (record kind {kind!r}); producer: {group['record'].get('producer', 'n/a')}"))
            continue
        if group.get("platform", "any") not in ("any", host_goos()):
            skipped.append((group["id"], f"recorded on {group['platform']}; this host is {host_goos()}"))
            continue
        selected.append(group)
    return selected, skipped


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("action", choices=("check", "record"))
    parser.add_argument("--group", action="append")
    parser.add_argument("--seal", action="store_true", help="stamp regeneration_verified into the manifest")
    args = parser.parse_args(argv)
    if shutil.which("go") is None:
        print("record.py needs a Go toolchain (this helper is not part of the Go-free gate)", file=sys.stderr)
        return 2
    registry = freeze.load(freeze.REGISTRY)
    selected, skipped = groups_to_run(registry, set(args.group) if args.group else None)
    for gid, why in skipped:
        print(f"SKIP {gid}: {why}")
    if args.action == "check" or args.action == "record":
        dirty = subprocess.run(["git", "status", "--porcelain", "--", "go.mod", "go.sum", "internal", "cmd", "guard", "scripts"],
                               cwd=ROOT, capture_output=True, text=True).stdout.strip()
        if dirty and args.action == "record":
            print("refusing to record with uncommitted Go/oracle sources:\n" + dirty, file=sys.stderr)
            return 2
    head = freeze.git("rev-parse", "HEAD")
    scratch = Path(tempfile.mkdtemp(prefix="symbrain-record."))
    worktree = scratch / "source"
    failures: list[str] = []
    verified: dict[str, dict] = {}
    try:
        subprocess.run(["git", "worktree", "add", "--quiet", "--detach", str(worktree), head], cwd=ROOT, check=True)
        for group in selected:
            gid = group["id"]
            try:
                if group["record"].get("kind") == "copy-of":
                    continue
                message = regenerate(group, worktree, scratch, "check" if args.action == "check" else "write")
            except RuntimeError as error:
                failures.append(f"{gid}: generator failed: {error}")
                continue
            drift = []
            if args.action == "check":
                if message:
                    drift.append(message)
            else:
                for rel in group["fixtures"]:
                    fresh, committed = worktree / rel, ROOT / rel
                    if not fresh.is_file():
                        drift.append(f"{rel}: generator did not write it")
                    elif not committed.is_file() or fresh.read_bytes() != committed.read_bytes():
                        committed.parent.mkdir(parents=True, exist_ok=True)
                        shutil.copyfile(fresh, committed)
                        drift.append(f"{rel}: rewritten")
            if drift and args.action == "check":
                failures.append(f"{gid}: " + "; ".join(drift))
            else:
                print(f"{'RECORDED' if drift else 'PASS'} {gid} ({len(group['fixtures'])} fixtures)")
                tc = group["record"].get("toolchain", "go.mod")
                tc = go_toolchain_default() if tc == "go.mod" else tc
                env = scrubbed_env({"GOTOOLCHAIN": tc})
                verified[gid] = {"method": ("generator's own check (" + ("-check" if group["record"].get("check_arg") else "go test without UPDATE" if group["record"].get("kind") == "go-test" else "normalised stdout compare") + ") run from HEAD in a scratch worktree"
                                           if args.action == "check" else "re-recorded from HEAD in a scratch worktree"),
                                 "at_commit": head, "go_version": go_version(env),
                                 "goos": host_goos(), "goarch": platform.machine().lower().replace("x86_64", "amd64").replace("aarch64", "arm64"),
                                 "date": datetime.date.today().isoformat(), "env": "scrubbed allow-list (no HERMES_*/credentials)"}
    finally:
        subprocess.run(["git", "worktree", "remove", "--force", str(worktree)], cwd=ROOT, capture_output=True)
        shutil.rmtree(scratch, ignore_errors=True)
    # copy-of groups follow their source
    for group in registry["groups"]:
        if group["record"].get("kind") == "copy-of" and (args.group is None or group["id"] in args.group):
            src, dst = ROOT / group["record"]["source"], ROOT / group["fixtures"][0]
            if args.action == "record" and src.read_bytes() != dst.read_bytes():
                shutil.copyfile(src, dst)
            if src.read_bytes() == dst.read_bytes():
                verified[group["id"]] = {"method": f"byte-identical to {group['record']['source']}", "at_commit": head}
            else:
                failures.append(f"{group['id']}: differs from {group['record']['source']}")
    if failures:
        print("\n".join("FAIL " + f for f in failures), file=sys.stderr)
        return 1
    if args.seal or args.action == "record":
        freeze.seal(only=set(verified) if args.action == "record" else None, verified=verified)
        print(f"sealed manifest ({len(verified)} regeneration receipts)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
