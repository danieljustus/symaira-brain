#!/usr/bin/env python3
"""CI-only native immutable parent build using the existing current Cargo cache."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import tempfile

from build_go import floor, output_path, run
from novel import ROOT, UNIX_PARENT, WINDOWS_PARENT, digest


def source_files(git, source, reference, env, report):
    tree = run([git, "ls-tree", "-r", "-z", reference], env, report, source)
    index = run([git, "ls-files", "--stage", "-z"], env, report, source)
    expected = {}
    for item in tree.split(b"\0"):
        if item:
            metadata, name = item.split(b"\t", 1)
            mode, kind, blob = metadata.split()
            if kind != b"blob" or mode not in (b"100644", b"100755"):
                raise ValueError("unexpected immutable parent source entry")
            expected[name] = (mode, blob)
    rows = []
    seen = {}
    for item in index.split(b"\0"):
        if not item:
            continue
        metadata, name = item.split(b"\t", 1)
        mode, blob, stage = metadata.split()
        if stage != b"0" or expected.get(name) != (mode, blob):
            raise ValueError("parent index/tree differs")
        path = source / os.fsdecode(name)
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("parent source must be regular files")
        payload = path.read_bytes()
        actual = hashlib.sha1(b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()
        if actual != blob.decode():
            raise ValueError("parent bytes differ from immutable tree")
        mode_bits = stat.S_IMODE(info.st_mode)
        if os.name != "nt" and mode_bits != (0o755 if mode == b"100755" else 0o644):
            raise ValueError("parent native permissions differ")
        seen[name] = (mode, blob)
        rows.append({"path_bytes_hex": name.hex(), "git_mode": mode.decode(), "native_mode": mode_bits,
                     "blob": actual, "sha256": digest(payload), "bytes": len(payload)})
    if seen != expected:
        raise ValueError("parent source file set differs")
    return rows


def build(args, current, saved, parent, report):
    target = args.target.resolve()
    expected = target / "debug" / ("symbrain.exe" if os.name == "nt" else "symbrain")
    if current != expected or not target.is_dir():
        raise ValueError("use only the existing current native CLI target/cache")
    floor((target, saved.parent))
    report["current_sha256"] = digest(current.read_bytes())
    shutil.copy2(current, saved)
    if digest(saved.read_bytes()) != report["current_sha256"]:
        raise ValueError("saved current native CLI bytes changed")
    git, cargo, rustup = (shutil.which(name) for name in ["git", "cargo", "rustup"])
    if not all([git, cargo, rustup]):
        raise ValueError("existing native Git/Cargo/Rustup SDK required")
    original_home = Path(os.environ["USERPROFILE"] if os.name == "nt" else os.environ["HOME"])
    with tempfile.TemporaryDirectory(prefix="skills-native-parent-794-", dir=saved.parent) as temporary:
        owned = Path(temporary)
        home = owned / "home"
        home.mkdir()
        template = owned / "template"
        template.mkdir()
        env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
        env.update(HOME=str(home), USERPROFILE=str(home), GIT_CONFIG_GLOBAL=os.devnull,
                   GIT_CONFIG_NOSYSTEM="1", GIT_TEMPLATE_DIR=str(template), GIT_TERMINAL_PROMPT="0",
                   CARGO_HOME=os.environ.get("CARGO_HOME", str(original_home / ".cargo")),
                   RUSTUP_HOME=os.environ.get("RUSTUP_HOME", str(original_home / ".rustup")),
                   CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0",
                   CARGO_NET_OFFLINE="true", SYMBRAIN_VERSION=os.environ.get("SYMBRAIN_VERSION", "dev"))
        toolchain = run([rustup, "show", "active-toolchain"], env, report, ROOT).decode().split()[0]
        env["RUSTUP_TOOLCHAIN"] = toolchain
        report["toolchain"] = toolchain
        report["candidate_head"] = run([git, "rev-parse", "HEAD"], env, report, ROOT).decode().strip()
        if run([git, "status", "--porcelain"], env, report, ROOT):
            raise ValueError("current candidate must be clean")
        source = owned / "source"
        run([git, "-c", "protocol.allow=never", "-c", "protocol.file.allow=always",
             "clone", "--shared", "--no-checkout", "--", ROOT, source], env, report)
        run([git, "config", "core.autocrlf", "false"], env, report, source)
        run([git, "checkout", "--detach", report["source_ref"]], env, report, source)
        if run([git, "rev-parse", "HEAD"], env, report, source).decode().strip() != report["source_ref"]:
            raise ValueError("wrong immutable native parent checkout")
        if (source / "rust-toolchain.toml").read_bytes() != (ROOT / "rust-toolchain.toml").read_bytes():
            raise ValueError("parent must use the same pinned current native Rust SDK")
        report["source_before"] = source_files(git, source, report["source_ref"], env, report)
        if run([git, "status", "--porcelain"], env, report, source):
            raise ValueError("parent fixture must be clean")
        floor((target, saved.parent))
        run([cargo, "build", "--locked", "--offline", "-p", "symbrain-cli", "--bin", "symbrain"],
            env, report, source, timeout=1800)
        shutil.copy2(current, parent)
        report["parent_sha256"] = digest(parent.read_bytes())
        report["source_after"] = source_files(git, source, report["source_ref"], env, report)
        if report["source_before"] != report["source_after"] or run(
                [git, "status", "--porcelain"], env, report, source):
            raise ValueError("parent build changed its source fixture")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["current", "target", "saved-current", "parent", "receipt"]:
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    os.umask(0o022)
    def interrupted(number, _frame):
        raise InterruptedError(f"SDK parent fixture interrupted by signal {number}")
    signal.signal(signal.SIGTERM, interrupted)
    current = args.current.resolve()
    saved, parent, receipt = (output_path(path) for path in [args.saved_current, args.parent, args.receipt])
    if len({saved, parent, receipt}) != 3:
        raise ValueError("all owned output paths must be distinct")
    report = {"source_ref": WINDOWS_PARENT if os.name == "nt" else UNIX_PARENT, "commands": [],
              "status": "failed", "restored_current": False, "existing_target": str(args.target.resolve()),
              "native_windows": os.name == "nt", "builder_sha256": digest(Path(__file__).read_bytes())}
    try:
        build(args, current, saved, parent, report)
        report["status"] = "passed"
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        print(report["error"])
    finally:
        try:
            if saved.exists():
                if digest(saved.read_bytes()) != report.get("current_sha256"):
                    raise ValueError("saved current bytes changed; do not replace target")
                shutil.copy2(saved, current)
                report["restored_current"] = digest(current.read_bytes()) == report["current_sha256"]
        except Exception as error:
            report["restoration_error"] = f"{type(error).__name__}: {error}"
        receipt.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    return 0 if report["status"] == "passed" and report["restored_current"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
