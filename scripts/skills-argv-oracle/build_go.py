#!/usr/bin/env python3
"""Build the fixed Skills oracle offline in an owned real Git-directory clone."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import stat
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
TOOLCHAIN = "go1.26.7"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def floor(paths):
    for path in paths:
        if shutil.disk_usage(path).free < 700 * 1024 * 1024:
            raise ValueError(f"below the 700MiB free-space floor: {path}")


def output_path(value):
    path = value.absolute()
    if path.exists() or path.is_symlink():
        raise ValueError(f"refuse to overwrite existing evidence: {path}")
    path = path.resolve()
    if not path.parent.is_dir() or path.is_relative_to(ROOT):
        raise ValueError("output must have an existing parent outside the checkout")
    return path


def stop_child(process, report):
    if process.poll() is not None:
        return
    if os.name == "nt":
        # The build has its own process group. Kill the still-owned Go/Git
        # process and its compiler descendants, not an image-name match.
        executable = Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"
        cleanup = subprocess.run([str(executable), "/PID", str(process.pid),
                                  "/T", "/F"], capture_output=True, timeout=30,
                                 check=False)
        report.setdefault("cleanup", []).append({
            "pid": process.pid, "exit": cleanup.returncode,
            "stdout_hex": cleanup.stdout.hex(), "stderr_hex": cleanup.stderr.hex()})
        if process.poll() is None:
            process.kill()
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
    process.wait(timeout=30)


def run(argv, env, report, cwd=None, timeout=120):
    record = {"argv": [str(arg) for arg in argv], "cwd": str(cwd) if cwd else None}
    report["commands"].append(record)
    options = {"creationflags": subprocess.CREATE_NEW_PROCESS_GROUP} if os.name == "nt" else {
        "start_new_session": True}
    process = subprocess.Popen(record["argv"], cwd=cwd, env=env,
                               stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE, **options)
    try:
        stdout, stderr = process.communicate(timeout=timeout)
    except BaseException:
        stop_child(process, report)
        stdout, stderr = process.communicate(timeout=30)
        record.update(exit=process.returncode, stdout_hex=stdout.hex(), stderr_hex=stderr.hex())
        raise
    record.update(exit=process.returncode, stdout_hex=stdout.hex(), stderr_hex=stderr.hex())
    if process.returncode:
        raise RuntimeError(f"command failed with exit {process.returncode}: {argv[0]}")
    return stdout


def source_map(git, source, env, report):
    index = run([git, "ls-files", "--stage", "-z"], env, report, source)
    tree = run([git, "ls-tree", "-r", "-z", FROZEN], env, report, source)
    expected = {}
    for item in tree.split(b"\0"):
        if item:
            metadata, name = item.split(b"\t", 1)
            mode, kind, blob = metadata.split()
            if kind != b"blob" or mode not in (b"100644", b"100755"):
                raise ValueError("unexpected frozen Skills source tree entry")
            expected[name] = (mode, blob)
    rows = []
    indexed = {}
    for item in index.split(b"\0"):
        if not item:
            continue
        metadata, name = item.split(b"\t", 1)
        mode, blob, stage = metadata.split()
        if stage != b"0" or (mode, blob) != expected.get(name):
            raise ValueError("frozen source index does not match the committed tree")
        path = source / os.fsdecode(name)
        info = path.lstat()
        if not stat.S_ISREG(info.st_mode) or path.is_symlink():
            raise ValueError("frozen source entry is not a regular owned file")
        payload = path.read_bytes()
        actual_blob = hashlib.sha1(b"blob " + str(len(payload)).encode() + b"\0" + payload).hexdigest()
        if actual_blob != blob.decode():
            raise ValueError("frozen source bytes do not match the committed blob")
        actual_mode = stat.S_IMODE(info.st_mode)
        # Windows has no POSIX executable bit. Its exact index mode and regular
        # file type are checked above; report native permission bits separately.
        if os.name != "nt" and actual_mode != (0o755 if mode == b"100755" else 0o644):
            raise ValueError("frozen source permissions differ from Git-declared modes")
        indexed[name] = (mode, blob)
        rows.append({"path_bytes_hex": name.hex(), "git_mode": mode.decode(),
                     "native_mode": actual_mode, "blob": actual_blob,
                     "sha256": digest(payload), "bytes": len(payload)})
    if indexed != expected:
        raise ValueError("frozen source index/tree file sets differ")
    return rows


def prepare_module_cache(go, git, source, env, report):
    """Fetch checksum-verified frozen dependencies before the offline proof."""
    preparation_env = dict(env, GOPROXY="https://proxy.golang.org",
                           GOSUMDB="sum.golang.org")
    report["module_cache_preparation"] = {
        "GOPROXY": preparation_env["GOPROXY"],
        "GOSUMDB": preparation_env["GOSUMDB"],
        "status": "started", "build_network": False}
    run([go, "mod", "download"], preparation_env, report, source, timeout=300)
    rows = source_map(git, source, env, report)
    report["source_after_cache_preparation"] = rows
    if rows != report["source_before"]:
        raise ValueError("module cache preparation changed frozen source bytes or modes")
    if run([git, "status", "--porcelain"], env, report, source):
        raise ValueError("frozen fixture must remain clean after cache preparation")
    report["module_cache_preparation"]["status"] = "passed"


def build(args, out, report):
    git = shutil.which("git")
    go = shutil.which("go")
    if not git or not go:
        raise ValueError("installed Git and pinned Go are required")
    for cache in (args.go_cache, args.module_cache):
        if not cache.is_dir():
            raise ValueError(f"explicit preexisting cache directory required: {cache}")
    with tempfile.TemporaryDirectory(prefix="skills-go-794-") as temporary:
        owned = Path(temporary).resolve()
        floor((owned, out.parent, args.go_cache, args.module_cache))
        if out.is_relative_to(owned):
            raise ValueError("oracle output must resolve outside the owned fixture")
        home = owned / "home"
        home.mkdir()
        template = owned / "empty-template"
        template.mkdir()
        env = {key: value for key, value in os.environ.items()
               if not key.startswith(("GO", "GIT_"))}
        env.update(HOME=str(home), USERPROFILE=str(home), XDG_CONFIG_HOME=str(home / "config"),
                   XDG_CACHE_HOME=str(home / "cache"), XDG_DATA_HOME=str(home / "data"),
                   GIT_CONFIG_GLOBAL=os.devnull, GIT_CONFIG_NOSYSTEM="1",
                   GIT_TEMPLATE_DIR=str(template), GIT_TERMINAL_PROMPT="0",
                   GOENV="off", GOTOOLCHAIN="local", GOPROXY="off", GOSUMDB="off",
                   GOWORK="off", CGO_ENABLED="0", GOMAXPROCS="2",
                   GOCACHE=str(args.go_cache.resolve()), GOMODCACHE=str(args.module_cache.resolve()),
                   GOPATH=str(home / "go"), GOTELEMETRYDIR=str(home / "telemetry"))
        report["environment_contract"] = {key: env[key] for key in [
            "HOME", "USERPROFILE", "GOENV", "GOTOOLCHAIN", "GOPROXY", "GOSUMDB", "GOWORK",
            "CGO_ENABLED", "GOMAXPROCS", "GOCACHE", "GOMODCACHE", "GOPATH", "GOTELEMETRYDIR",
            "GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM", "GIT_TEMPLATE_DIR", "GIT_TERMINAL_PROMPT"]}
        version = run([go, "version"], env, report).decode().strip().split()
        if len(version) != 4 or version[:3] != ["go", "version", TOOLCHAIN]:
            raise ValueError("installed Go must be exactly go1.26.7; automatic download is disabled")
        report["candidate_head"] = run([git, "rev-parse", "HEAD"], env, report, ROOT).decode().strip()
        if run([git, "status", "--porcelain"], env, report, ROOT):
            raise ValueError("candidate source checkout must be clean")
        source = owned / "source"
        run([git, "-c", "protocol.allow=never", "-c", "protocol.file.allow=always",
             "clone", "--shared", "--no-checkout", "--", ROOT, source], env, report)
        run([git, "config", "core.autocrlf", "false"], env, report, source)
        run([git, "config", "core.filemode", "false" if os.name == "nt" else "true"], env, report, source)
        run([git, "checkout", "--detach", FROZEN], env, report, source)
        if not (source / ".git").is_dir():
            raise ValueError("owned oracle needs a real .git directory")
        if run([git, "rev-parse", "HEAD"], env, report, source).decode().strip() != FROZEN:
            raise ValueError("unexpected oracle revision")
        report["source_before"] = source_map(git, source, env, report)
        if run([git, "status", "--porcelain"], env, report, source):
            raise ValueError("frozen fixture must be clean before build")
        if args.prepare_module_cache:
            prepare_module_cache(go, git, source, env, report)
        run([go, "mod", "verify"], env, report, source, timeout=300)
        floor((owned, out.parent, args.go_cache))
        run([go, "build", "-p=2", "-mod=readonly", "-buildvcs=true", "-o", out,
             "./cmd/symbrain"], env, report, source, timeout=900)
        report["source_after"] = source_map(git, source, env, report)
        if report["source_before"] != report["source_after"]:
            raise ValueError("oracle build changed source bytes or modes")
        if run([git, "status", "--porcelain"], env, report, source):
            raise ValueError("frozen fixture must remain clean after build")
        information = run([go, "version", "-m", out], env, report).decode()
        settings = {line.strip().split("\t", 1)[1] for line in information.splitlines()
                    if line.strip().startswith("build\t")}
        if information.splitlines()[0].split()[-1] != TOOLCHAIN or not {
                "vcs=git", f"vcs.revision={FROZEN}", "vcs.modified=false", "CGO_ENABLED=0"} <= settings:
            raise ValueError("oracle build lacks exact pinned unmodified VCS metadata")
        report.update(go_build_info=information, output_sha256=digest(out.read_bytes()),
                      output_bytes=out.stat().st_size, git_directory=True, status="passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--go-cache", type=Path, required=True)
    parser.add_argument("--module-cache", type=Path, required=True)
    parser.add_argument("--prepare-module-cache", action="store_true",
                        help="explicitly fetch verified frozen dependencies before offline verification/build")
    args = parser.parse_args()
    os.umask(0o022)
    # These handlers are scoped to this SDK fixture program. Product CLI signal
    # behavior and embedded/custom-writer contracts are not modified.
    def interrupted(number, _frame):
        raise InterruptedError(f"SDK fixture interrupted by signal {number}")
    signal.signal(signal.SIGTERM, interrupted)
    out, receipt = output_path(args.out), output_path(args.receipt)
    if out == receipt:
        raise ValueError("binary and receipt outputs must be distinct")
    report = {"frozen": FROZEN, "toolchain": TOOLCHAIN, "builder_sha256": digest(Path(__file__).read_bytes()),
              "output": str(out), "commands": [], "status": "failed", "native_windows": os.name == "nt"}
    try:
        build(args, out, report)
    except Exception as error:
        report["error"] = f"{type(error).__name__}: {error}"
        print(report["error"], file=sys.stderr)
        return 1
    finally:
        receipt.write_text(json.dumps(report, indent=2, ensure_ascii=True) + "\n", encoding="utf-8")
    print(f"Built pinned clean Skills oracle: {report['output_sha256']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
