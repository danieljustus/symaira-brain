#!/usr/bin/env python3
"""Real frozen-Go/native Skills argv comparison; Windows uses CreateProcessW."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
RETAINED = ROOT / "migration/evidence/skills-preflight-793/maine3-integration/symaira-pr794-main31-raw-flag-process.json"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def cases():
    if os.name != "nt":
        records = json.loads(RETAINED.read_text(encoding='utf-8'))["results"]
        assert len(records) == 315
        return [(f"retained-{i:03}", [bytes.fromhex(arg) for arg in row["raw_args_hex"]])
                for i, row in enumerate(records)]
    result = []
    # Python's Windows Popen uses CreateProcessW. A surrogatepass UTF-16
    # representation below records the actual code units supplied, not U+FFFD.
    for value in ["\ud800", "\udc00", "\ud800\ud800", "\udc00\ud800",
                  "\ud800x\udc00", "\ufffd", "\U0001f600", "é"]:
        for spelling in [f"--bad{value}", f"--bad{value}=ignored", f"-{value}",
                         f"---{value}", f"----{value}", f"--={value}"]:
            for prefix in [[], ["--target=opencode"], ["--dry-run=true"]]:
                result.append((f"wide-flag-{len(result):03}", prefix + [spelling]))
        for pad in ["", " ", "\u00a0", "\u3000"]:
            padded = pad + value + pad
            for flag in ["target", "scope"]:
                for inline in [False, True]:
                    args = [f"--{flag}={padded}"] if inline else [f"--{flag}", padded]
                    result.append((f"wide-value-{len(result):03}", args))
        result.append((f"wide-bool-{len(result):03}", [f"--dry-run={value}"]))
        result.append((f"wide-normalized-value-{len(result):03}", ["--target", "--" + value]))
        result.append((f"wide-help-{len(result):03}", ["--help", f"--bad{value}"]))
        result.append((f"wide-terminator-{len(result):03}", ["--target=bad", "--", f"--bad{value}"]))
        result.append((f"wide-positional-stop-{len(result):03}", ["--target=bad", "operand", f"--bad{value}"]))
    return result


def snapshot(root):
    result = {}
    for path in sorted(root.rglob("*")):
        info = path.lstat()
        row = {"mode": stat.S_IMODE(info.st_mode), "kind": stat.S_IFMT(info.st_mode)}
        if path.is_symlink():
            row["link"] = os.readlink(path)
        elif path.is_file():
            row["sha256"] = digest(path)
        result[str(path.relative_to(root))] = row
    return result


def run(binary, argv, owned):
    for name in ["home", "config", "data", "cache", "project", "empty-bin"]:
        (owned / name).mkdir()
    env = {"HOME": str(owned / "home"), "USERPROFILE": str(owned / "home"),
           "XDG_CONFIG_HOME": str(owned / "config"), "XDG_DATA_HOME": str(owned / "data"),
           "XDG_CACHE_HOME": str(owned / "cache"), "PATH": str(owned / "empty-bin"),
           "SYMBRAIN_GO_BINARY": str(owned / "absent-go"), "LANG": "C.UTF-8", "TZ": "UTC"}
    if os.name == "nt":
        for name in ["SystemRoot", "WINDIR"]:
            if name in os.environ:
                env[name] = os.environ[name]
        args = [str(binary), "skills", "sync", "--json", *argv]
    else:
        args = [os.fsencode(binary), b"skills", b"sync", b"--json", *argv]
    before = snapshot(owned)
    process = subprocess.run(args, cwd=owned / "project", env=env,
                             capture_output=True, timeout=4, check=False)
    return {"exit_code": process.returncode, "stdout_hex": process.stdout.hex(),
            "stderr_hex": process.stderr.hex(), "state_before": before,
            "state_after": snapshot(owned)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--go", type=Path, required=True)
    parser.add_argument("--rust", type=Path, required=True)
    parser.add_argument("--parent", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--negative-control", choices=["flag-input", "quoted-value", "normalization"])
    args = parser.parse_args()
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT).strip()
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True, encoding='utf-8').strip()
    names = subprocess.check_output(["git", "ls-files", "rust", "Cargo.toml", "Cargo.lock"],
                                    cwd=ROOT, text=True, encoding='utf-8').splitlines()
    sources = {name: digest(ROOT / name) for name in names
               if name.endswith((".rs", ".toml")) or name == "Cargo.lock"}
    for name, sha in sources.items():
        immutable = subprocess.check_output(["git", "show", f"{head}:{name}"], cwd=ROOT)
        assert hashlib.sha256(immutable).hexdigest() == sha
    binaries = {"go": args.go.resolve(), "rust": args.rust.resolve()}
    if args.parent:
        binaries["parent"] = args.parent.resolve()
    go_build_info = subprocess.check_output(["go", "version", "-m", str(binaries["go"])], text=True, encoding='utf-8')
    assert "go1.26.7" in go_build_info
    assert f"vcs.revision={FROZEN}" in go_build_info and "vcs.modified=false" in go_build_info
    historical = json.loads(RETAINED.read_text(encoding='utf-8'))["results"] if os.name != "nt" else []
    report = {"go_build_info": go_build_info, "candidate_head": head, "candidate_dirty": False,
              "candidate_sources_sha256": sources, "frozen_go_commit": FROZEN,
              "platform": platform.platform(), "native_windows": os.name == "nt",
              "launch": "Python Popen/CreateProcessW wide args" if os.name == "nt" else "POSIX raw byte argv",
              "binaries_sha256": {name: digest(path) for name, path in binaries.items()},
              "negative_control": args.negative_control, "results": []}
    with tempfile.TemporaryDirectory(prefix="skills-argv-793-") as temp:
        scratch = Path(temp)
        # Execute immutable copies. A caller's target is neither written nor cleaned.
        copies = {}
        for name, binary in binaries.items():
            copies[name] = scratch / (name + (".exe" if os.name == "nt" else ""))
            shutil.copy2(binary, copies[name])
            assert digest(copies[name]) == report["binaries_sha256"][name]
        original = cases()
        if args.negative_control:
            flag = {"flag-input": "--different", "quoted-value": "--target=different",
                    "normalization": "----different"}[args.negative_control]
            original = [("actual-input-control", ["--bad"] if os.name == "nt" else [b"--bad"])]
        for index, (case_id, argv) in enumerate(original):
            row = {"id": case_id, "raw_args_hex": [
                value.encode("utf-16-le", "surrogatepass").hex() if os.name == "nt" else value.hex()
                for value in argv], "raw_encoding": "UTF-16LE" if os.name == "nt" else "bytes"}
            for name, binary in copies.items():
                owned = scratch / f"{index}-{name}"
                owned.mkdir()
                passed = argv
                if name == "rust" and args.negative_control:
                    passed = [flag] if os.name == "nt" else [flag.encode()]
                row[name] = run(binary, passed, owned)
            row["matched"] = row["go"] == row["rust"]
            row["historical_go_preserved"] = True
            if historical and not args.negative_control:
                expected = historical[index]["go"]
                row["historical_go_preserved"] = all(row["go"][key] == value for key, value in expected.items())
            row["readonly"] = all(value["state_before"] == value["state_after"]
                                  for name, value in row.items() if name in copies)
            report["results"].append(row)
    report["total"] = len(report["results"])
    report["matched"] = sum(row["matched"] for row in report["results"])
    report["readonly"] = all(row["readonly"] for row in report["results"])
    report["historical_go_preserved"] = all(row["historical_go_preserved"] for row in report["results"])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding='utf-8')
    print(f'{report["matched"]}/{report["total"]} exact actual pairs; readonly={report["readonly"]}')
    return 0 if report["matched"] == report["total"] and report["readonly"] and report["historical_go_preserved"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
