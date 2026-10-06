#!/usr/bin/env python3
"""Execute the immutable supplemental Skills plan at its actual native scope."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import stat
import subprocess
import tempfile

from inherited_output import OWNER_PATHS, qualify

ROOT = Path(__file__).resolve().parents[2]
PLAN = ROOT / "migration/evidence/skills-preflight-793/windows-wide-argv/original-70b-static/symaira-skills794-wide-argv-independent-novel-plan.json"
PLAN_SHA = "5478c095a73d0332baa987dfae3dea6d3c3863104841be7b4d689edfba762bce"
GO_REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
WINDOWS_PARENT = "01f41906e2e021db3c693ec97617bb701e4dad8a"
UNIX_PARENT = "faa8f12f6c77aad694003c9fa8c5e66d94919ba0"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def selected_cases(windows):
    payload = PLAN.read_bytes()
    if digest(payload) != PLAN_SHA:
        raise ValueError("immutable supplemental plan changed")
    cases = json.loads(payload)["novel_cases"]
    wanted = {"all-native", "native-Windows" if windows else "native-Unix"}
    chosen = [case for case in cases if case["platform"] in wanted]
    inherited = [case for case in chosen if case["id"].startswith("inherited-global")]
    if (len(chosen), len(inherited)) != ((112, 3) if windows else (53, 2)):
        raise ValueError("unexpected immutable native plan cardinality")
    return chosen


def arguments(case, windows):
    if "args" in case:
        return case["args"] if windows else [value.encode("utf-8") for value in case["args"]]
    if "args_bytes_hex" in case:
        if windows:
            raise ValueError("Unix byte vector cannot be admitted as Windows evidence")
        return [bytes.fromhex(value) for value in case["args_bytes_hex"]]
    values = [bytes.fromhex(value).decode("utf-16-le", "surrogatepass")
              for value in case["args_utf16le_hex"]]
    return values if windows else [value.encode("utf-8") for value in values]


def snapshot(root):
    rows = []
    for path in [root, *sorted(root.rglob("*"))]:
        info = path.lstat()
        row = {"relative_path_bytes_hex": os.fsencode(path.relative_to(root)).hex(),
               "mode": stat.S_IMODE(info.st_mode), "kind": stat.S_IFMT(info.st_mode),
               "size": info.st_size, "mtime_ns": info.st_mtime_ns}
        if path.is_symlink():
            row["link_target_bytes_hex"] = os.fsencode(os.readlink(path)).hex()
        elif path.is_file():
            payload = path.read_bytes()
            row.update(bytes_hex=payload.hex(), sha256=digest(payload))
        rows.append(row)
    return rows


def seed(root):
    directories = ["home", "config", "data", "cache", "project", "empty-bin", "config/symbrain",
                   "config/symskills", "data/symbrain/skills/library/broken", "home/.claude/skills/owned"]
    for directory in directories:
        (root / directory).mkdir(parents=True, exist_ok=True)
    payloads = [("config/symbrain/config.toml", b"[malformed Brain"),
                ("config/symskills/config.toml", b"[malformed Skills"),
                ("project/.symbrain.toml", b"[malformed project"),
                ("data/symbrain/skills/library/broken/SKILL.md", b"---\nnot valid: [\n---\nowned malformed skill"),
                ("home/.claude/skills/owned/.symskills.json", b"{owned-malformed-marker"),
                ("project/owned.txt", b"unchanged\x00raw\xff")]
    for name, payload in payloads:
        path = root / name
        path.write_bytes(payload)
        path.chmod(0o640)
    for path in [root, *root.rglob("*")]:
        os.utime(path, ns=(10**18, 10**18))


def execute(binary, argv, root):
    seed(root)
    env = {"HOME": str(root / "home"), "USERPROFILE": str(root / "home"),
           "XDG_CONFIG_HOME": str(root / "config"), "XDG_DATA_HOME": str(root / "data"),
           "XDG_CACHE_HOME": str(root / "cache"), "PATH": str(root / "empty-bin"),
           "SYMBRAIN_GO_BINARY": str(root / "absent-fallback"), "LANG": "C.UTF-8", "TZ": "UTC"}
    if os.name == "nt":
        for name in ["SystemRoot", "WINDIR"]:
            if name in os.environ:
                env[name] = os.environ[name]
        command = [str(binary), "skills", "sync", "--json", *argv]
    else:
        command = [os.fsencode(binary), b"skills", b"sync", b"--json", *argv]
    before = snapshot(root)
    try:
        process = subprocess.run(command, cwd=root / "project", env=env, capture_output=True,
                                 timeout=5, check=False)
        result = {"exit": process.returncode, "stdout_hex": process.stdout.hex(),
                  "stderr_hex": process.stderr.hex(), "timed_out": False}
    except subprocess.TimeoutExpired as error:
        result = {"exit": None, "stdout_hex": (error.stdout or b"").hex(),
                  "stderr_hex": (error.stderr or b"").hex(), "timed_out": True}
    after = snapshot(root)
    return {**result, "before": before, "after": after,
            "readonly": before == after, "environment": env}


def parent_provenance(parent, proof, current_hash):
    payload = proof.read_bytes()
    data = json.loads(payload)
    parent_hash = digest(parent.read_bytes())
    reference = WINDOWS_PARENT if os.name == "nt" else UNIX_PARENT
    if "executables" in data:
        if os.name == "nt" or data["head"] != reference or not data["roundtrip_verified"]:
            raise ValueError("wrong native original parent archive")
        role = next(row for row in data["executables"]
                    if row["original_path"].endswith("/target/debug/symbrain"))
        archive = data["unique"][role["sha256"]]
        original = gzip.decompress(Path(archive["archive"]).read_bytes())
        if digest(original) != parent_hash or role["sha256"] != parent_hash:
            raise ValueError("parent bytes do not match the actual original CLI archive")
    else:
        if not (data["status"] == "passed" and data["source_ref"] == reference
                and data["parent_sha256"] == parent_hash and data["current_sha256"] == current_hash
                and data["source_before"] == data["source_after"] and data["restored_current"]):
            raise ValueError("parent build/source/current restoration proof is incomplete")
    parent_sources = {}
    current_sources = {}
    for name in OWNER_PATHS:
        old = subprocess.check_output(["git", "show", f"{reference}:{name}"], cwd=ROOT)
        now = (ROOT / name).read_bytes()
        parent_sources[name] = old
        current_sources[name] = now
    owners = qualify(reference, parent_sources, current_sources)
    return {"source_ref": reference, "proof_path": str(proof), "proof_sha256": digest(payload),
            "parent_sha256": parent_hash, "inherited_output_owner": owners}


def same_output(left, right):
    return all(left[key] == right[key] for key in ["exit", "stdout_hex", "stderr_hex"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["go", "rust", "parent", "parent-proof", "out"]:
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--negative-control", choices=["flag-input", "parent-input"])
    args = parser.parse_args()
    os.umask(0o022)
    if args.out.exists() or args.out.is_symlink():
        raise ValueError("never overwrite original observations")
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT):
        raise ValueError("candidate source must be clean")
    binaries = {name: getattr(args, name).resolve() for name in ["go", "rust", "parent"]}
    info = subprocess.check_output(["go", "version", "-m", binaries["go"]], text=True, encoding='utf-8')
    if (info.splitlines()[0].split()[-1] != "go1.26.7" or
            f"vcs.revision={GO_REF}" not in info or "vcs.modified=false" not in info):
        raise ValueError("pinned clean frozen Go metadata required")
    hashes = {name: digest(path.read_bytes()) for name, path in binaries.items()}
    provenance = parent_provenance(binaries["parent"], args.parent_proof, hashes["rust"])
    sources = subprocess.check_output(["git", "ls-files", "-z", "rust", "Cargo.toml", "Cargo.lock",
                                      "rust-toolchain.toml", "scripts/skills-argv-oracle",
                                      ".github/workflows/ci.yml"], cwd=ROOT).split(b"\0")
    cases = selected_cases(os.name == "nt")
    if args.negative_control:
        inherited = args.negative_control == "parent-input"
        cases = [next(case for case in cases if case["id"].startswith("inherited-global") == inherited)]
    report = {"candidate_head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT).decode().strip(),
              "candidate_dirty": False, "plan_sha256": PLAN_SHA, "platform": platform.platform(),
              "native_windows": os.name == "nt", "launch": "CreateProcessW UTF16" if os.name == "nt" else "POSIX bytes",
              "binaries_sha256": hashes, "go_build_info": info, "parent_provenance": provenance,
              "candidate_sources_sha256": {os.fsdecode(name): digest((ROOT / os.fsdecode(name)).read_bytes())
                                           for name in sources if name},
              "negative_control": args.negative_control, "cases": []}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="skills-novel-794-") as temporary:
        scratch = Path(temporary)
        copies = {}
        for name, binary in binaries.items():
            copies[name] = scratch / (name + (".exe" if os.name == "nt" else ""))
            shutil.copy2(binary, copies[name])
            if digest(copies[name].read_bytes()) != hashes[name]:
                raise ValueError("immutable binary copy changed")
        for index, case in enumerate(cases):
            argv = arguments(case, os.name == "nt")
            inherited = case["id"].startswith("inherited-global")
            row = {"id": case["id"], "scope": "actual-parent" if inherited else "required-Go-native",
                   "args_hex": [value.encode("utf-16-le", "surrogatepass").hex() if os.name == "nt" else value.hex()
                                for value in argv], "raw_encoding": "UTF16LE" if os.name == "nt" else "bytes"}
            for name, binary in copies.items():
                root = scratch / f"{index}-{name}"
                root.mkdir()
                passed = argv
                if ((args.negative_control == "flag-input" and name == "rust") or
                        (args.negative_control == "parent-input" and name == "parent")):
                    mutant = "--output=actual-parent-input-control" if inherited else "--actual-native-input-control"
                    passed = [mutant] if os.name == "nt" else [mutant.encode()]
                row[name] = execute(binary, passed, root)
            row["Go_native_exact"] = same_output(row["go"], row["rust"])
            row["current_actual_parent_exact"] = same_output(row["rust"], row["parent"])
            row["readonly"] = all(row[name]["readonly"] for name in copies)
            row["accepted"] = row["readonly"] and all(not row[name]["timed_out"] for name in copies) and (
                row["current_actual_parent_exact"] if inherited else row["Go_native_exact"])
            if inherited:
                row["accepted"] &= all(row[name]["exit"] == 2 and row[name]["stdout_hex"] == ""
                                       and bytes.fromhex(row[name]["stderr_hex"]).startswith(b"symbrain: ")
                                       for name in ["rust", "parent"])
            report["cases"].append(row)
            args.out.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    report["summary"] = {"total": len(cases), "accepted": sum(row["accepted"] for row in report["cases"]),
                         "required": sum(row["scope"] == "required-Go-native" for row in report["cases"]),
                         "inherited": sum(row["scope"] == "actual-parent" for row in report["cases"]),
                         "readonly": all(row["readonly"] for row in report["cases"])}
    args.out.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    print(json.dumps(report["summary"]))
    return 0 if report["summary"]["total"] == report["summary"]["accepted"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
