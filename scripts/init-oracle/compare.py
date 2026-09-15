#!/usr/bin/env python3
"""Bounded standalone binary oracle and parity comparator for ``symbrain init``."""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
from typing import Any, Callable

PINNED_COMMIT_SHA = "d53c3824e7d6771fabefd62a18dd3c5e50c3c37d"
TIMEOUT_BUILD_SECONDS = 180.0
TIMEOUT_CASE_SECONDS = 10.0

MANDATORY_GO_SOURCE_FILES: tuple[str, ...] = (
    "cmd/symbrain/cmd_init.go",
    "cmd/symbrain/main.go",
    "internal/xdg/xdg.go",
    "go.mod",
)

REQUIRED_CASE_IDS: tuple[str, ...] = (
    "fresh",
    "rerun_preserves_edits",
    "absolute_xdg",
    "relative_xdg_data_cache",
    "relative_config_ignored",
    "fallback_home",
    "missing_home_with_explicit_xdg",
    "missing_home_without_explicit_xdg",
    "unknown_flag",
    "malformed_flag_triple_dash",
    "malformed_flag_empty_name",
    "help",
    "help_equals_false",
    "positionals",
    "terminator",
    "dir_as_file_skip",
    "symlink_to_existing_skip",
    "dangling_symlink_replacement",
    "deterministic_mkdir_collision",
    "partial_write_before_self_symlink_error",
)


def sha256_bytes(data: bytes) -> str:
    """Computes hex SHA-256 digest of bytes."""
    return hashlib.sha256(data).hexdigest()


def sha256_file(path: Path) -> str:
    """Computes hex SHA-256 digest of a file."""
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


class ProcessResult:
    """Bounded process execution result."""

    def __init__(
        self,
        argv: list[str],
        returncode: int | None,
        stdout: bytes,
        stderr: bytes,
        timed_out: bool = False,
    ) -> None:
        self.argv = argv
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr
        self.timed_out = timed_out


def run_bounded(
    argv: list[str],
    *,
    cwd: Path,
    env: dict[str, str],
    stdin: bytes | None = None,
    timeout: float = TIMEOUT_CASE_SECONDS,
) -> ProcessResult:
    """Runs a subprocess in an isolated process group with bounded timeout and cleanup."""
    out_file = tempfile.NamedTemporaryFile(prefix="init-oracle-out-", delete=False)
    err_file = tempfile.NamedTemporaryFile(prefix="init-oracle-err-", delete=False)
    out_name, err_name = out_file.name, err_file.name
    out_file.close()
    err_file.close()
    timed_out = False
    p: subprocess.Popen[bytes] | None = None
    try:
        with open(out_name, "wb") as out, open(err_name, "wb") as err:
            kwargs: dict[str, Any] = {
                "cwd": cwd,
                "env": env,
                "stdin": subprocess.PIPE if stdin is not None else None,
                "stdout": out,
                "stderr": err,
            }
            if os.name == "posix":
                kwargs["start_new_session"] = True
            p = subprocess.Popen(argv, **kwargs)
            try:
                p.communicate(input=stdin, timeout=timeout)
            except subprocess.TimeoutExpired:
                timed_out = True
                if os.name == "posix":
                    try:
                        os.killpg(p.pid, signal.SIGTERM)
                    except ProcessLookupError:
                        pass
                else:
                    p.terminate()
                try:
                    p.wait(timeout=1.0)
                except subprocess.TimeoutExpired:
                    if os.name == "posix":
                        try:
                            os.killpg(p.pid, signal.SIGKILL)
                        except ProcessLookupError:
                            pass
                    else:
                        p.kill()
                    try:
                        p.wait(timeout=2.0)
                    except subprocess.TimeoutExpired:
                        pass
            rc = p.returncode
        out_bytes = Path(out_name).read_bytes()
        err_bytes = Path(err_name).read_bytes()
        return ProcessResult(
            argv=argv,
            returncode=rc,
            stdout=out_bytes,
            stderr=err_bytes,
            timed_out=timed_out,
        )
    finally:
        if p is not None and p.poll() is None:
            if os.name == "posix":
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
            else:
                try:
                    p.kill()
                except Exception:
                    pass
        for name in (out_name, err_name):
            try:
                os.unlink(name)
            except FileNotFoundError:
                pass


def verify_commit_sha(value: str) -> str:
    if not re.fullmatch(r"[0-9a-f]{40}", value):
        raise ValueError(f"commit SHA must be 40-character lowercase hex: {value!r}")
    return value


def validate_archive_path_containment(dest_dir: Path, relative_path_str: str) -> Path:
    """Validates that relative_path_str does not escape dest_dir."""
    dest_resolved = dest_dir.resolve()
    target_resolved = (dest_dir / relative_path_str).resolve()
    try:
        target_resolved.relative_to(dest_resolved)
    except ValueError:
        raise RuntimeError(
            f"Archive member path escapes destination directory: {relative_path_str!r}"
        )
    if target_resolved == dest_resolved:
        raise RuntimeError(
            f"Archive member path resolves to destination root: {relative_path_str!r}"
        )
    return target_resolved


def git_archive_tar(
    repo_root: Path,
    commit: str,
    paths: list[str] | None = None,
    extra_env: dict[str, str] | None = None,
) -> bytes:
    """Runs git archive with explicit overrides ensuring deterministic LF tar stream."""
    verify_commit_sha(commit)
    cmd = [
        "git",
        "-c",
        "core.autocrlf=false",
        "-c",
        "core.eol=lf",
        "archive",
        "--format=tar",
        commit,
    ]
    if paths:
        cmd.extend(paths)
    env = dict(os.environ)
    if extra_env:
        env.update(extra_env)
    return subprocess.check_output(cmd, cwd=repo_root, env=env)


def get_git_tree_inventory(repo_root: Path, commit: str) -> dict[str, dict[str, str]]:
    """Returns a map of path -> {'mode': mode, 'type': type, 'object_sha': sha} for all objects at commit."""
    verify_commit_sha(commit)
    raw = subprocess.check_output(
        ["git", "ls-tree", "-r", "-z", commit],
        cwd=repo_root,
    )
    inventory: dict[str, dict[str, str]] = {}
    if not raw:
        return inventory
    entries = raw.split(b"\0")
    for entry in entries:
        if not entry:
            continue
        meta, tab, path_bytes = entry.partition(b"\t")
        if not tab:
            continue
        meta_parts = meta.decode("ascii", errors="replace").split()
        if len(meta_parts) != 3:
            continue
        mode, obj_type, obj_sha = meta_parts
        path_str = path_bytes.decode("utf-8", errors="replace")
        inventory[path_str] = {
            "mode": mode,
            "type": obj_type,
            "object_sha": obj_sha,
        }
    return inventory


def get_git_blobs_batch(repo_root: Path, obj_shas: list[str]) -> dict[str, bytes]:
    """Fetches raw blob bytes from git cat-file --batch for the given object SHAs."""
    if not obj_shas:
        return {}
    proc = subprocess.Popen(
        ["git", "cat-file", "--batch"],
        cwd=repo_root,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    input_data = b"".join(f"{sha}\n".encode("ascii") for sha in obj_shas)
    stdout, stderr = proc.communicate(input=input_data)
    if proc.returncode != 0:
        raise RuntimeError(
            f"git cat-file --batch failed (rc={proc.returncode}): {stderr.decode('utf-8', errors='replace')}"
        )

    blobs: dict[str, bytes] = {}
    stream = io.BytesIO(stdout)
    for expected_sha in obj_shas:
        header_line = stream.readline()
        if not header_line:
            raise RuntimeError(f"Unexpected EOF while reading blob {expected_sha}")
        parts = header_line.decode("ascii", errors="replace").strip().split()
        if len(parts) != 3 or parts[1] != "blob":
            raise RuntimeError(
                f"Invalid git cat-file output for {expected_sha}: {header_line!r}"
            )
        sha, _, size_str = parts
        size = int(size_str)
        content = stream.read(size)
        newline = stream.read(1)
        blobs[sha] = content
    return blobs


def extract_and_verify_go_sources(
    repo_root: Path,
    commit: str,
    dest_dir: Path,
) -> tuple[dict[str, str], list[dict[str, Any]]]:
    """Extracts Go sources from archived commit and verifies all regular files against git blobs and inventory."""
    verify_commit_sha(commit)
    if dest_dir.exists():
        shutil.rmtree(dest_dir)
    dest_dir.mkdir(parents=True, exist_ok=True)

    # 1. Get full git tree inventory
    tree_inventory = get_git_tree_inventory(repo_root, commit)

    # 2. Check mandatory files exist in git tree inventory
    for mandatory in MANDATORY_GO_SOURCE_FILES:
        if mandatory not in tree_inventory:
            raise RuntimeError(
                f"Mandatory source file {mandatory!r} missing from git tree at commit {commit}"
            )

    # Filter to all regular files (blobs) in tree inventory
    regular_tree_files = {
        path: item
        for path, item in tree_inventory.items()
        if item["type"] == "blob"
    }

    # Fetch all git blob bytes for regular files
    unique_shas = sorted(list({item["object_sha"] for item in regular_tree_files.values()}))
    blob_map = get_git_blobs_batch(repo_root, unique_shas)

    # 3. Extract tar archive from git_archive_tar
    archive_bytes = git_archive_tar(repo_root, commit)

    manifest_dict: dict[str, str] = {}
    source_manifest_list: list[dict[str, Any]] = []

    with tarfile.open(fileobj=io.BytesIO(archive_bytes), mode="r:") as tar:
        for member in tar:
            if not member.isfile():
                continue

            member_path = validate_archive_path_containment(dest_dir, member.name)
            f = tar.extractfile(member)
            if f is None:
                continue
            content = f.read()
            member_path.parent.mkdir(parents=True, exist_ok=True)
            member_path.write_bytes(content)

            file_sha = sha256_bytes(content)
            manifest_dict[member.name] = file_sha
            source_manifest_list.append(
                {
                    "path": member.name,
                    "sha256": file_sha,
                    "size_bytes": len(content),
                }
            )

    source_manifest_list.sort(key=lambda x: x["path"])

    # 4. Verify all mandatory files are present in extracted manifest
    for mandatory in MANDATORY_GO_SOURCE_FILES:
        if mandatory not in manifest_dict:
            raise RuntimeError(
                f"Mandatory source file {mandatory!r} missing from extracted archive"
            )

    # 5. Verify ALL regular tree files against extracted files
    for path, meta in regular_tree_files.items():
        if path not in manifest_dict:
            raise RuntimeError(
                f"Tracked regular file {path!r} was not extracted from git archive"
            )
        expected_bytes = blob_map[meta["object_sha"]]
        expected_sha = sha256_bytes(expected_bytes)
        if manifest_dict[path] != expected_sha:
            raise RuntimeError(
                f"Extracted file {path!r} hash {manifest_dict[path]} "
                f"does not match git blob hash {expected_sha}"
            )
        disk_bytes = (dest_dir / path).read_bytes()
        if disk_bytes != expected_bytes:
            raise RuntimeError(
                f"Extracted disk content for {path!r} does not match git blob bytes"
            )

    # 6. Verify in reverse that no unexpected files were extracted
    for extracted_path in manifest_dict:
        if extracted_path not in regular_tree_files:
            raise RuntimeError(
                f"Extracted file {extracted_path!r} is not a regular file in git tree inventory"
            )

    return manifest_dict, source_manifest_list


def verify_autocrlf_hostility_resilience(repo_root: Path, commit: str) -> None:
    """Verifies that git_archive_tar overrides hostile inherited GIT_CONFIG_COUNT settings."""
    hostile_env = {
        "GIT_CONFIG_COUNT": "2",
        "GIT_CONFIG_KEY_0": "core.autocrlf",
        "GIT_CONFIG_VALUE_0": "true",
        "GIT_CONFIG_KEY_1": "core.eol",
        "GIT_CONFIG_VALUE_1": "crlf",
    }
    tar_bytes = git_archive_tar(
        repo_root,
        commit,
        paths=["cmd/symbrain/cmd_init.go"],
        extra_env=hostile_env,
    )
    with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as tar:
        f = tar.extractfile("cmd/symbrain/cmd_init.go")
        if f is None:
            raise RuntimeError("failed to extract cmd_init.go in autocrlf test")
        content = f.read()
        if b"\r\n" in content:
            raise RuntimeError(
                "hostile GIT_CONFIG_COUNT contaminated tar extraction with CRLF line endings"
            )


def build_go_reference_binary(
    go_source_dir: Path,
    output_binary_path: Path,
    build_cache_dir: Path,
) -> dict[str, str]:
    """Builds the Go reference binary from extracted sources."""
    output_binary_path.parent.mkdir(parents=True, exist_ok=True)
    build_cache_dir.mkdir(parents=True, exist_ok=True)

    env = dict(os.environ)
    env["GOCACHE"] = str(build_cache_dir / "gocache")
    env["GOPATH"] = str(build_cache_dir / "gopath")
    env["CGO_ENABLED"] = "0"

    go_version_proc = subprocess.run(
        ["go", "version"],
        capture_output=True,
        text=True,
        check=True,
    )
    go_version = go_version_proc.stdout.strip()

    build_cmd = ["go", "build", "-o", str(output_binary_path), "./cmd/symbrain"]
    try:
        result = subprocess.run(
            build_cmd, cwd=go_source_dir, env=env, capture_output=True,
            timeout=TIMEOUT_BUILD_SECONDS,
        )
    except subprocess.TimeoutExpired as error:
        (output_binary_path.parent / "build-stdout.log").write_bytes(error.stdout or b"")
        (output_binary_path.parent / "build-stderr.log").write_bytes(error.stderr or b"")
        raise
    (output_binary_path.parent / "build-stdout.log").write_bytes(result.stdout)
    (output_binary_path.parent / "build-stderr.log").write_bytes(result.stderr)
    result.check_returncode()

    if not output_binary_path.is_file():
        raise RuntimeError(f"Go build failed to produce {output_binary_path}")

    return {
        "go_version": go_version,
        "binary_sha256": sha256_file(output_binary_path),
    }


def capture_fs_manifest(root: Path) -> list[dict[str, Any]]:
    """Captures a complete sorted filesystem manifest of all entries under root."""
    entries: list[dict[str, Any]] = []
    if not root.exists():
        return entries

    for dirpath, dirnames, filenames in os.walk(root, followlinks=False):
        dirpath_p = Path(dirpath)
        for d in sorted(dirnames):
            p = dirpath_p / d
            rel = p.relative_to(root).as_posix()
            if p.is_symlink():
                target = os.readlink(p)
                entries.append(
                    {
                        "path": rel,
                        "type": "symlink",
                        "target": target,
                    }
                )
            else:
                st = p.stat()
                entries.append(
                    {
                        "path": rel,
                        "type": "dir",
                        "mode": oct(stat.S_IMODE(st.st_mode)),
                    }
                )

        for f in sorted(filenames):
            p = dirpath_p / f
            rel = p.relative_to(root).as_posix()
            if p.is_symlink():
                target = os.readlink(p)
                entries.append(
                    {
                        "path": rel,
                        "type": "symlink",
                        "target": target,
                    }
                )
            else:
                st = p.stat()
                content = p.read_bytes()
                entries.append(
                    {
                        "path": rel,
                        "type": "file",
                        "size": len(content),
                        "sha256": sha256_bytes(content),
                        "mode": oct(stat.S_IMODE(st.st_mode)),
                    }
                )

    entries.sort(key=lambda x: x["path"])
    return entries


class CaseRunner:
    """Defines and executes parity cases comparing Go and Rust binaries sequentially."""

    def __init__(
        self,
        go_binary: Path,
        rust_binary: Path,
        base_dir: Path,
        case_timeout: float = TIMEOUT_CASE_SECONDS,
    ) -> None:
        self.go_binary = go_binary
        self.rust_binary = rust_binary
        self.base_dir = base_dir
        self.case_timeout = case_timeout

    def run_case(
        self,
        case_id: str,
        name: str,
        setup_seed: Callable[[Path], None],
        args: list[str],
        env_overrides: dict[str, str | None] | None = None,
        working_sub_dir: str | None = None,
    ) -> dict[str, Any]:
        """Executes a single test case sequentially against Go and Rust at the same root."""
        seed_dir = self.base_dir / "seeds" / case_id
        if seed_dir.exists():
            shutil.rmtree(seed_dir)
        seed_dir.mkdir(parents=True, exist_ok=True)
        try:
            setup_seed(seed_dir)
        except Exception as e:
            return {
                "case_id": case_id,
                "name": name,
                "status": "fail",
                "diffs": [f"fixture setup failed: {e}"],
                "go": {
                    "argv": [],
                    "exit_code": None,
                    "timed_out": False,
                    "stdout": "",
                    "stdout_b64": "",
                    "stderr": "",
                    "stderr_b64": "",
                    "manifest": [],
                    "entries_count": 0,
                },
                "rust": {
                    "argv": [],
                    "exit_code": None,
                    "timed_out": False,
                    "stdout": "",
                    "stdout_b64": "",
                    "stderr": "",
                    "stderr_b64": "",
                    "manifest": [],
                    "entries_count": 0,
                },
            }

        run_root = self.base_dir / "runs" / case_id / "root"

        # --- GO EXECUTION ---
        if run_root.exists():
            shutil.rmtree(run_root)
        run_root.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(seed_dir, run_root, symlinks=True)

        cwd_path = run_root / working_sub_dir if working_sub_dir else run_root
        cwd_path.mkdir(parents=True, exist_ok=True)

        go_env = self._build_env(run_root, env_overrides, is_rust=False)
        go_cmd = [str(self.go_binary), "init", *args]
        go_proc = run_bounded(go_cmd, cwd=cwd_path, env=go_env, timeout=self.case_timeout)
        go_manifest = capture_fs_manifest(run_root)

        # --- RUST EXECUTION ---
        if run_root.exists():
            shutil.rmtree(run_root)
        run_root.parent.mkdir(parents=True, exist_ok=True)
        shutil.copytree(seed_dir, run_root, symlinks=True)

        cwd_path = run_root / working_sub_dir if working_sub_dir else run_root
        cwd_path.mkdir(parents=True, exist_ok=True)

        rust_env = self._build_env(run_root, env_overrides, is_rust=True)
        rust_cmd = [str(self.rust_binary), "init", *args]
        rust_proc = run_bounded(rust_cmd, cwd=cwd_path, env=rust_env, timeout=self.case_timeout)
        rust_manifest = capture_fs_manifest(run_root)

        # Clean run root
        if run_root.exists():
            shutil.rmtree(run_root)

        # --- COMPARE PARITY ---
        diffs: list[str] = []

        if go_proc.timed_out or rust_proc.timed_out:
            if go_proc.timed_out and rust_proc.timed_out:
                diffs.append("both Go and Rust executions timed out")
            elif go_proc.timed_out:
                diffs.append("Go execution timed out")
            else:
                diffs.append("Rust execution timed out")

        if go_proc.returncode != rust_proc.returncode:
            diffs.append(
                f"returncode mismatch: Go={go_proc.returncode}, Rust={rust_proc.returncode}"
            )

        if go_proc.stdout != rust_proc.stdout:
            diffs.append(
                f"stdout mismatch:\n--- Go stdout ({len(go_proc.stdout)} B) ---\n"
                f"{go_proc.stdout.decode('utf-8', errors='replace')}\n"
                f"--- Rust stdout ({len(rust_proc.stdout)} B) ---\n"
                f"{rust_proc.stdout.decode('utf-8', errors='replace')}"
            )

        if go_proc.stderr != rust_proc.stderr:
            diffs.append(
                f"stderr mismatch:\n--- Go stderr ({len(go_proc.stderr)} B) ---\n"
                f"{go_proc.stderr.decode('utf-8', errors='replace')}\n"
                f"--- Rust stderr ({len(rust_proc.stderr)} B) ---\n"
                f"{rust_proc.stderr.decode('utf-8', errors='replace')}"
            )

        if go_manifest != rust_manifest:
            diffs.append(
                f"filesystem manifest mismatch:\n--- Go manifest ---\n"
                f"{json.dumps(go_manifest, indent=2)}\n--- Rust manifest ---\n"
                f"{json.dumps(rust_manifest, indent=2)}"
            )

        status = "pass" if not diffs else "fail"

        return {
            "case_id": case_id,
            "name": name,
            "status": status,
            "diffs": diffs,
            "go": {
                "argv": go_cmd,
                "exit_code": go_proc.returncode,
                "timed_out": go_proc.timed_out,
                "stdout": go_proc.stdout.decode("utf-8", errors="replace"),
                "stdout_b64": base64.b64encode(go_proc.stdout).decode("ascii"),
                "stderr": go_proc.stderr.decode("utf-8", errors="replace"),
                "stderr_b64": base64.b64encode(go_proc.stderr).decode("ascii"),
                "manifest": go_manifest,
                "entries_count": len(go_manifest),
            },
            "rust": {
                "argv": rust_cmd,
                "exit_code": rust_proc.returncode,
                "timed_out": rust_proc.timed_out,
                "stdout": rust_proc.stdout.decode("utf-8", errors="replace"),
                "stdout_b64": base64.b64encode(rust_proc.stdout).decode("ascii"),
                "stderr": rust_proc.stderr.decode("utf-8", errors="replace"),
                "stderr_b64": base64.b64encode(rust_proc.stderr).decode("ascii"),
                "manifest": rust_manifest,
                "entries_count": len(rust_manifest),
            },
        }

    def _build_env(
        self,
        run_root: Path,
        overrides: dict[str, str | None] | None,
        is_rust: bool,
    ) -> dict[str, str]:
        home_path = run_root / "home"
        env: dict[str, str] = {
            "HOME": str(home_path),
            "USERPROFILE": str(home_path),
            "HOMEDRIVE": "",
            "HOMEPATH": "",
            "TMPDIR": str(self.base_dir / "tmp"),
            "TEMP": str(self.base_dir / "tmp"),
            "TMP": str(self.base_dir / "tmp"),
            "LANG": "C.UTF-8",
            "LC_ALL": "C.UTF-8",
        }
        if sys.platform == "win32" or os.name == "nt":
            for key in (
                "SystemRoot",
                "SYSTEMROOT",
                "windir",
                "WINDIR",
                "SystemDrive",
                "SYSTEMDRIVE",
                "PATHEXT",
                "ComSpec",
                "COMSPEC",
            ):
                if key in os.environ:
                    env[key] = os.environ[key]
            if "SystemRoot" not in env and "SYSTEMROOT" not in env:
                env["SystemRoot"] = r"C:\Windows"
            if "WINDIR" not in env and "windir" not in env:
                env["WINDIR"] = env.get("SystemRoot", r"C:\Windows")
            env["PATH"] = os.environ.get("PATH", r"C:\Windows\System32;C:\Windows")
            if is_rust:
                env["SYMBRAIN_GO_BINARY"] = r"C:\nonexistent\symbrain-go-binary.exe"
        else:
            env["PATH"] = "/usr/bin:/bin:/usr/sbin:/sbin"
            if is_rust:
                env["SYMBRAIN_GO_BINARY"] = "/nonexistent/symbrain-go-binary"

        # Clear default XDG to simulate default resolution
        for var in [
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
            "XDG_RUNTIME_DIR",
            "SYMBRAIN_DEFAULT_PROFILE",
            "SYMBRAIN_AUDIT_ENABLED",
        ]:
            env.pop(var, None)

        if overrides:
            for k, v in overrides.items():
                if v is None:
                    env.pop(k, None)
                else:
                    env[k] = str(v).replace("{RUN_ROOT}", str(run_root))

        return env


class CaseSpec:
    """Specification of a parity test case."""

    def __init__(
        self,
        case_id: str,
        name: str,
        setup_seed: Callable[[Path], None],
        args: list[str],
        env_overrides: dict[str, str | None] | None = None,
        working_sub_dir: str | None = None,
    ) -> None:
        self.case_id = case_id
        self.name = name
        self.setup_seed = setup_seed
        self.args = args
        self.env_overrides = env_overrides
        self.working_sub_dir = working_sub_dir


def get_case_specs() -> list[CaseSpec]:
    """Returns the declared battery of parity test cases."""
    specs: list[CaseSpec] = []

    # 1. Fresh initialization
    def setup_fresh(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "fresh",
            "Fresh initialization in clean environment",
            setup_fresh,
            [],
        )
    )

    # 2. Re-run preserves user edits
    def setup_rerun(root: Path) -> None:
        config_dir = root / "home" / ".config" / "symbrain"
        config_dir.mkdir(parents=True, exist_ok=True)
        (config_dir / "config.toml").write_text("# user edits preserved\n")

    specs.append(
        CaseSpec(
            "rerun_preserves_edits",
            "Re-run skips and preserves existing configuration",
            setup_rerun,
            [],
        )
    )

    # 3. Absolute XDG variables
    def setup_abs_xdg(root: Path) -> None:
        (root / "custom_config").mkdir(parents=True, exist_ok=True)
        (root / "custom_data").mkdir(parents=True, exist_ok=True)
        (root / "custom_cache").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "absolute_xdg",
            "Absolute XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_CACHE_HOME paths",
            setup_abs_xdg,
            [],
            env_overrides={
                "XDG_CONFIG_HOME": "{RUN_ROOT}/custom_config",
                "XDG_DATA_HOME": "{RUN_ROOT}/custom_data",
                "XDG_CACHE_HOME": "{RUN_ROOT}/custom_cache",
            },
        )
    )

    # 4. Relative XDG data and cache
    def setup_rel_xdg(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "relative_xdg_data_cache",
            "Relative XDG_DATA_HOME and XDG_CACHE_HOME are honored",
            setup_rel_xdg,
            [],
            env_overrides={
                "XDG_DATA_HOME": "rel_data_dir",
                "XDG_CACHE_HOME": "rel_cache_dir",
            },
        )
    )

    # 5. Relative XDG config ignored
    def setup_rel_config(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "relative_config_ignored",
            "Relative XDG_CONFIG_HOME is ignored per specification",
            setup_rel_config,
            [],
            env_overrides={
                "XDG_CONFIG_HOME": "rel_config_dir",
            },
        )
    )

    # 6. Fallback to HOME
    def setup_fallback_home(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "fallback_home",
            "Fallback to standard HOME directories",
            setup_fallback_home,
            [],
        )
    )

    # 7. Missing HOME with explicit XDG
    def setup_missing_home_with_xdg(root: Path) -> None:
        (root / "custom_config").mkdir(parents=True, exist_ok=True)
        (root / "custom_data").mkdir(parents=True, exist_ok=True)
        (root / "custom_cache").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "missing_home_with_explicit_xdg",
            "Missing HOME with explicit XDG succeeds without error",
            setup_missing_home_with_xdg,
            [],
            env_overrides={
                "HOME": None,
                "USERPROFILE": None,
                "XDG_CONFIG_HOME": "{RUN_ROOT}/custom_config",
                "XDG_DATA_HOME": "{RUN_ROOT}/custom_data",
                "XDG_CACHE_HOME": "{RUN_ROOT}/custom_cache",
            },
        )
    )

    # 8. Missing HOME without explicit XDG
    def setup_missing_home_no_xdg(root: Path) -> None:
        pass

    specs.append(
        CaseSpec(
            "missing_home_without_explicit_xdg",
            "Missing HOME without explicit XDG fails with $HOME is not defined",
            setup_missing_home_no_xdg,
            [],
            env_overrides={
                "HOME": None,
                "USERPROFILE": None,
            },
        )
    )

    # 9. Unknown flag
    def setup_unknown_flag(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "unknown_flag",
            "Unknown flag rejected with usage",
            setup_unknown_flag,
            ["--unknown-custom-flag"],
        )
    )

    # 10. Malformed flag triple dash
    def setup_malformed_triple_dash(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "malformed_flag_triple_dash",
            "Malformed flag ---bogus rejected with usage",
            setup_malformed_triple_dash,
            ["---bogus"],
        )
    )

    # 11. Malformed flag empty name
    def setup_malformed_empty_name(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "malformed_flag_empty_name",
            "Malformed flag --=x rejected with usage",
            setup_malformed_empty_name,
            ["--=x"],
        )
    )

    # 12. Help flag
    def setup_help(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "help",
            "Help flag prints usage and exits with code 2",
            setup_help,
            ["--help"],
        )
    )

    # 13. Help flag with explicit false
    def setup_help_equals_false(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "help_equals_false",
            "Help flag with explicit false --help=false prints usage and exits with code 2",
            setup_help_equals_false,
            ["--help=false"],
        )
    )

    # 14. Positionals accepted
    def setup_pos(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "positionals",
            "Positional arguments stop flag parsing and are ignored",
            setup_pos,
            ["extra_arg1", "extra_arg2"],
        )
    )

    # 15. Flag terminator --
    def setup_terminator(root: Path) -> None:
        (root / "home").mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "terminator",
            "Flag terminator -- is accepted and stops parsing",
            setup_terminator,
            ["--", "--ignored_flag"],
        )
    )

    # 16. Dir-as-file skip
    def setup_dir_as_file(root: Path) -> None:
        cfg_dir = root / "home" / ".config" / "symbrain" / "config.toml"
        cfg_dir.mkdir(parents=True, exist_ok=True)

    specs.append(
        CaseSpec(
            "dir_as_file_skip",
            "Directory located at file path is skipped as existing",
            setup_dir_as_file,
            [],
        )
    )

    # 17. Symlink to existing file skip
    def setup_symlink_existing(root: Path) -> None:
        home = root / "home"
        profiles = home / ".config" / "symbrain" / "profiles"
        profiles.mkdir(parents=True, exist_ok=True)
        real_file = home / "real_personal.toml"
        real_file.write_text("# real target\n")
        symlink_target = profiles / "personal.toml"
        os.symlink(str(real_file), str(symlink_target))

    specs.append(
        CaseSpec(
            "symlink_to_existing_skip",
            "Symlink pointing to existing file is skipped and preserved",
            setup_symlink_existing,
            [],
        )
    )

    # 18. Dangling symlink replacement
    def setup_dangling_symlink(root: Path) -> None:
        home = root / "home"
        profiles = home / ".config" / "symbrain" / "profiles"
        profiles.mkdir(parents=True, exist_ok=True)
        symlink_target = profiles / "restricted.toml"
        os.symlink("nonexistent_target.toml", str(symlink_target))

    specs.append(
        CaseSpec(
            "dangling_symlink_replacement",
            "Dangling symlink is replaced with real atomic file",
            setup_dangling_symlink,
            [],
        )
    )

    # 19. Deterministic mkdir collision
    def setup_mkdir_collision(root: Path) -> None:
        config_parent = root / "home" / ".config"
        config_parent.mkdir(parents=True, exist_ok=True)
        # Create a file at .config/symbrain so profiles/ dir cannot be created inside it
        (config_parent / "symbrain").write_text("collision file\n")

    specs.append(
        CaseSpec(
            "deterministic_mkdir_collision",
            "File collision at directory path fails mkdir and exits 1",
            setup_mkdir_collision,
            [],
        )
    )

    # 20. Partial write before self symlink error
    def setup_self_symlink(root: Path) -> None:
        home = root / "home"
        profiles = home / ".config" / "symbrain" / "profiles"
        profiles.mkdir(parents=True, exist_ok=True)
        # Cyclic symlink: restricted.toml -> restricted.toml
        os.symlink("restricted.toml", str(profiles / "restricted.toml"))

    specs.append(
        CaseSpec(
            "partial_write_before_self_symlink_error",
            "Partial creation before ELOOP cyclic symlink error",
            setup_self_symlink,
            [],
        )
    )

    return specs


def validate_case_inventory(
    declared_specs: list[CaseSpec],
    case_results: list[dict[str, Any]],
) -> None:
    """Validates that declared specs and executed case results are complete, unique, and match."""
    if not case_results:
        raise ValueError("No test cases were executed; inventory is empty")

    executed_ids = [c["case_id"] for c in case_results]
    declared_ids = [s.case_id for s in declared_specs]

    seen_ids: set[str] = set()
    duplicates: list[str] = []
    for cid in executed_ids:
        if cid in seen_ids:
            duplicates.append(cid)
        seen_ids.add(cid)
    if duplicates:
        raise ValueError(f"Duplicate case IDs found in execution results: {duplicates}")

    missing_required = [cid for cid in REQUIRED_CASE_IDS if cid not in seen_ids]
    if missing_required:
        raise ValueError(f"Missing required case IDs in execution: {missing_required}")

    if executed_ids != declared_ids:
        raise ValueError(
            f"Declared case inventory {declared_ids} does not match executed inventory {executed_ids}"
        )


def run_all_cases(runner: CaseRunner) -> list[dict[str, Any]]:
    """Runs the full battery of parity test cases and validates inventory."""
    specs = get_case_specs()
    results: list[dict[str, Any]] = []

    for spec in specs:
        res = runner.run_case(
            case_id=spec.case_id,
            name=spec.name,
            setup_seed=spec.setup_seed,
            args=spec.args,
            env_overrides=spec.env_overrides,
            working_sub_dir=spec.working_sub_dir,
        )
        results.append(res)

    validate_case_inventory(specs, results)
    return results


def build_parser() -> argparse.ArgumentParser:
    """Constructs the argument parser for compare.py."""
    parser = argparse.ArgumentParser(
        description="Oracle and differential comparator for symbrain init."
    )
    parser.add_argument(
        "--rust-binary",
        type=Path,
        required=True,
        help="Path to the compiled symbrain Rust binary",
    )
    parser.add_argument(
        "--output",
        type=Path,
        required=True,
        help="Path to write the JSON comparison report",
    )
    parser.add_argument(
        "--commit",
        type=str,
        default=PINNED_COMMIT_SHA,
        choices=[PINNED_COMMIT_SHA],
        help=f"Pinned commit SHA for Go reference source extraction (frozen at {PINNED_COMMIT_SHA})",
    )
    parser.add_argument(
        "--repo-root",
        type=Path,
        default=Path(__file__).resolve().parents[2],
        help="Repository root directory",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.commit != PINNED_COMMIT_SHA:
        raise ValueError(f"commit must be frozen at pinned commit {PINNED_COMMIT_SHA}")

    repo_root = args.repo_root.resolve()
    target_oracle_dir = repo_root / "target" / "init-oracle"
    target_oracle_dir.mkdir(parents=True, exist_ok=True)

    # 1. Verify hostile autocrlf resilience
    verify_autocrlf_hostility_resilience(repo_root, args.commit)

    # 2. Extract and verify Go sources
    go_src_dir = target_oracle_dir / "go-source"
    manifest_dict, source_manifest = extract_and_verify_go_sources(
        repo_root,
        args.commit,
        go_src_dir,
    )

    # 3. Always build Go reference binary from verified extracted sources (.exe on Windows)
    go_bin_name = "symbrain-go.exe" if sys.platform == "win32" or os.name == "nt" else "symbrain-go"
    go_binary_path = target_oracle_dir / "go-bin" / go_bin_name
    go_build_info = build_go_reference_binary(
        go_src_dir,
        go_binary_path,
        target_oracle_dir / "go-cache",
    )

    # 4. Check Rust binary
    rust_binary_path = args.rust_binary.resolve()
    if not rust_binary_path.is_file():
        raise FileNotFoundError(f"Rust binary not found: {rust_binary_path}")

    rustc_version = (
        subprocess.check_output(["rustc", "--version"], text=True).strip()
    )
    rust_build_info = {
        "rustc_version": rustc_version,
        "binary_sha256": sha256_file(rust_binary_path),
    }

    # 5. Run Parity Cases
    runner = CaseRunner(
        go_binary=go_binary_path,
        rust_binary=rust_binary_path,
        base_dir=target_oracle_dir,
    )
    case_results = run_all_cases(runner)

    passed_count = sum(1 for c in case_results if c["status"] == "pass")
    failed_count = len(case_results) - passed_count

    report: dict[str, Any] = {
        "schema_version": 1,
        "tool": "init-oracle",
        "pinned_commit": args.commit,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "host": {
            "os": sys.platform,
            "system": platform.system(),
            "release": platform.release(),
            "architecture": platform.machine(),
            "platform": platform.platform(),
        },
        "host_os": sys.platform,
        "host_arch": platform.machine(),
        "go_info": go_build_info,
        "rust_info": rust_build_info,
        "source_manifest": source_manifest,
        "cases_total": len(case_results),
        "cases_passed": passed_count,
        "cases_failed": failed_count,
        "passed": failed_count == 0,
        "results": case_results,
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with open(args.output, "w", encoding="utf-8") as f:
        json.dump(report, f, indent=2)

    print(f"init-oracle: {passed_count}/{len(case_results)} cases passed.")
    if failed_count > 0:
        for c in case_results:
            if c["status"] != "pass":
                print(f"FAIL: {c['case_id']} - {c['name']}")
                for d in c["diffs"]:
                    print(f"  {d}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
