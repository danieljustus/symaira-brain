"""Owned compilation sources and configuration admission, before SDK children."""
import hashlib
import json
import os
from pathlib import Path
import shutil


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def configurations(checkout):
    # Cargo starts at its cwd, then walks parents. This deliberately rejects
    # both tracked and untracked configuration; the builder emits none there.
    for parent in (checkout, *checkout.parents):
        for name in ("config", "config.toml"):
            path = parent / ".cargo" / name
            if path.exists() or path.is_symlink():
                raise ValueError("unbound Cargo ancestor configuration: " + str(path))


def physical(checkout, tracked, role):
    expected = set(tracked)
    if role == "go":
        expected.add("internal/memory/contextassembler/zz_native_engine_oracle_test.go")
    roots = [checkout / "rust"] if role == "rust" else [checkout]
    for root in roots:
        for parent, dirs, files in os.walk(root, followlinks=False):
            dirs[:] = [d for d in dirs if d != ".git"]
            for name in dirs + files:
                path = Path(parent) / name
                relative = path.relative_to(checkout).as_posix()
                if relative == ".git":
                    # Linked worktree metadata selects the independently
                    # checked Git identity; it is never staged as source.
                    continue
                if path.is_symlink():
                    raise ValueError("unbound compilation symlink: " + relative)
                if path.is_file() and relative not in expected:
                    # A complete physical census, including ignored inputs,
                    # prevents automatic build.rs and include_* admission.
                    raise ValueError("unexpected physical compilation input: " + relative)


def stage(checkout, selected, destination):
    destination.mkdir(parents=True, exist_ok=False)
    for name, expected in selected.items():
        original = checkout / name
        if original.is_symlink() or sha(original) != expected:
            raise ValueError("source changed before staging: " + name)
        target = destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(original, target)
        target.chmod(original.stat().st_mode & 0o777)
        if sha(target) != expected:
            raise ValueError("staged source differs: " + name)
    configurations(destination)


def environment(output, go_sdk, rust_sdk, target):
    # Never inherit PATH, Cargo/Rust/Go flags, wrappers, workspace settings,
    # Git configuration, compiler include paths, or an operator home/cache.
    env = {k: os.environ[k] for k in ("SystemRoot", "SYSTEMROOT", "WINDIR") if k in os.environ}
    env.update(HOME=str(output / "home"), USERPROFILE=str(output / "home"),
               TMPDIR=str(output / "tmp"), TMP=str(output / "tmp"), TEMP=str(output / "tmp"),
               PATH=str(rust_sdk / "bin") + os.pathsep + str(go_sdk / "bin"),
               GOCACHE=str(output / "go-cache"), GOMODCACHE=str(output / "go-modcache"),
               GOROOT=str(go_sdk), GOWORK="off", GOENV="off", GOTOOLCHAIN="local",
               GOFLAGS="-mod=readonly", GOPROXY="off",
               GOSUMDB="off", GOTELEMETRY="off", CGO_ENABLED="0", GOMAXPROCS="2",
               CARGO_HOME=str(output / "cargo-home"), CARGO_NET_OFFLINE="true",
               CARGO_NET_GIT_FETCH_WITH_CLI="false", CARGO_TARGET_DIR=str(target),
               CARGO_BUILD_JOBS="2", CARGO_INCREMENTAL="0", CARGO_PROFILE_DEV_DEBUG="0",
               CARGO_PROFILE_TEST_DEBUG="0", RUSTC=str(rust_sdk / "bin/rustc"),
               RUSTDOC=str(rust_sdk / "bin/rustdoc"), RUSTC_WRAPPER="", RUSTC_WORKSPACE_WRAPPER="",
               GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=str(output / "empty-git-config"))
    return env


def artifact(stdout, target, manifest):
    artifacts = []
    for line in stdout.splitlines():
        event = json.loads(line)
        if event.get("reason") == "compiler-artifact" and event.get("target", {}).get("name") == "engine_probe" and event.get("target", {}).get("kind") == ["example"]:
            if event.get("fresh") is not False:
                raise ValueError("current engine_probe must be compiled, not an unbound cached artifact")
            path = event.get("executable")
            if Path(event.get("manifest_path", "")).resolve() != manifest.resolve():
                raise ValueError("Cargo artifact belongs to another package")
            if path:
                artifacts.append(Path(path).resolve())
    if len(artifacts) != 1 or not artifacts[0].is_relative_to(target.resolve()) or not artifacts[0].is_file():
        raise ValueError("one actual Cargo engine_probe executable in owned target required")
    return artifacts[0]
