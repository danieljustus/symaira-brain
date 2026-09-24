#!/usr/bin/env python3
"""Build deterministic dual Go/Rust release archives without fabricating signatures.

The builder creates archive, checksum, SPDX and signature-input manifests.  It
never writes ``.sig`` or ``.pem`` files.  A signing job must add those files and
flip each implementation's signature-input manifest to ``signed: true`` before
``verify.py`` can accept the result.
"""
from __future__ import annotations

import argparse
import gzip
import hashlib
import json
import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path
from typing import Any, Sequence

if str(Path(__file__).resolve().parent) not in sys.path:
    sys.path.insert(0, str(Path(__file__).resolve().parent))

from binary_smoke import check_binary

ROOT = Path(__file__).resolve().parents[2]
EXTERNAL_BASE_ENV = "SYMAIRA_EXTERNAL_BASE"
EXTERNAL_RUNTIME_ROOT = Path("/Volumes/1TB_NVMe_SN850X")
DEFAULT_EXTERNAL_BASE = EXTERNAL_RUNTIME_ROOT / "Dev" / "Symaira_Dev" / "builds" / "symaira-brain"
TARGETS: dict[str, tuple[str, str, str]] = {
    "darwin-amd64": ("darwin", "amd64", "x86_64-apple-darwin"),
    "darwin-arm64": ("darwin", "arm64", "aarch64-apple-darwin"),
    "linux-amd64": ("linux", "amd64", "x86_64-unknown-linux-gnu"),
    "linux-arm64": ("linux", "arm64", "aarch64-unknown-linux-gnu"),
    "windows-amd64": ("windows", "amd64", "x86_64-pc-windows-msvc"),
    "windows-arm64": ("windows", "arm64", "aarch64-pc-windows-msvc"),
}


def _external_path(value: str | Path, mounted_root: Path, *, name: str) -> Path:
    path = Path(value).expanduser()
    if not path.is_absolute():
        raise RuntimeError(f"{name} must be an absolute path under {mounted_root}: {path}")
    try:
        resolved = path.resolve(strict=False)
    except OSError as error:
        raise RuntimeError(f"{name} is invalid: {path}: {error}") from error
    if not resolved.is_relative_to(mounted_root):
        raise RuntimeError(f"{name} must be under mounted NVMe volume {mounted_root}: {resolved}")
    path.mkdir(parents=True, exist_ok=True)
    resolved = path.resolve(strict=True)
    if not resolved.is_dir() or not os.access(resolved, os.W_OK | os.X_OK):
        raise RuntimeError(f"{name} must name a writable directory: {resolved}")
    return resolved


def external_environment(env: dict[str, str]) -> dict[str, str]:
    """Keep direct macOS build, temp, and release paths on the encrypted NVMe."""
    if sys.platform != "darwin" or env.get("CI"):
        return env
    try:
        mounted_root = EXTERNAL_RUNTIME_ROOT.resolve(strict=True)
    except OSError as error:
        raise RuntimeError(f"required NVMe runtime volume is unavailable: {EXTERNAL_RUNTIME_ROOT}: {error}") from error
    if not mounted_root.is_dir() or not mounted_root.is_mount():
        raise RuntimeError(f"required NVMe runtime volume is not mounted: {mounted_root}")
    base = _external_path(
        env.get(EXTERNAL_BASE_ENV, str(DEFAULT_EXTERNAL_BASE)), mounted_root, name=EXTERNAL_BASE_ENV
    )
    for name, path in {
        "TMPDIR": base / "tmp",
        "TMP": base / "tmp",
        "TEMP": base / "tmp",
        "GOTMPDIR": base / "go-tmp",
        "GOPATH": base / "gopath",
        "GOCACHE": base / "go-cache",
        "GOMODCACHE": base / "go-mod-cache",
        "GOTELEMETRYDIR": base / "go-telemetry",
        "CARGO_HOME": base / "cargo-home",
        "CARGO_TARGET_DIR": base / "cargo-target",
        "PYTHONPYCACHEPREFIX": base / "python-cache",
    }.items():
        env[name] = str(_external_path(path, mounted_root, name=name))
    env[EXTERNAL_BASE_ENV] = str(base)
    return env


def release_output(root: Path, requested: Path, env: dict[str, str]) -> Path:
    if sys.platform == "darwin" and not env.get("CI"):
        output = Path(env[EXTERNAL_BASE_ENV]) / requested if not requested.is_absolute() else requested
        resolved = output.resolve(strict=False)
        if not resolved.is_relative_to(EXTERNAL_RUNTIME_ROOT.resolve(strict=True)):
            raise RuntimeError(f"--output must be under mounted NVMe volume {EXTERNAL_RUNTIME_ROOT}: {resolved}")
        return resolved
    return (root / requested).resolve() if not requested.is_absolute() else requested.resolve()


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def host_target() -> str:
    system = platform.system().lower()
    machine = platform.machine().lower()
    os_name = {"darwin": "darwin", "linux": "linux", "windows": "windows"}.get(system)
    arch = {"x86_64": "amd64", "amd64": "amd64", "aarch64": "arm64", "arm64": "arm64"}.get(machine)
    if os_name is None or arch is None:
        raise RuntimeError(f"unsupported release host: {system}/{machine}")
    return f"{os_name}-{arch}"


def run(command: Sequence[str], *, cwd: Path, env: dict[str, str]) -> None:
    print("+", " ".join(command), flush=True)
    subprocess.run(command, cwd=cwd, env=env, check=True)


def try_run(command: Sequence[str], *, cwd: Path, env: dict[str, str]) -> bool:
    try:
        run(command, cwd=cwd, env=env)
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"BLOCKED: {' '.join(command)} ({error})", file=sys.stderr, flush=True)
        return False
    return True


def go_binary(root: Path, staging: Path, target: str, version: str, env: dict[str, str]) -> Path | None:
    os_name, arch, _ = TARGETS[target]
    path = staging / "go-binaries" / target / ("symbrowse.exe" if os_name == "windows" else "symbrowse")
    path.parent.mkdir(parents=True, exist_ok=True)
    build_env = dict(env, GOOS=os_name, GOARCH=arch, CGO_ENABLED="0")
    command = [
        "go",
        "build",
        "-trimpath",
        "-ldflags",
        f"-s -w -X main.version={version.removeprefix('v')}",
        "-o",
        str(path),
        "./cmd/symbrowse",
    ]
    return path if try_run(command, cwd=root, env=build_env) and path.is_file() else None


def _cargo_target_dir(root: Path, env: dict[str, str]) -> Path:
    configured = env.get("CARGO_TARGET_DIR")
    if not configured:
        if sys.platform == "darwin" and not env.get("CI"):
            return Path(external_environment(env)["CARGO_TARGET_DIR"])
        return root / "target"
    path = Path(configured)
    if sys.platform == "darwin" and not env.get("CI") and not path.is_absolute():
        raise RuntimeError("CARGO_TARGET_DIR must be an absolute NVMe path on direct macOS runs")
    return path if path.is_absolute() else root / path


def rust_binary(root: Path, staging: Path, target: str, version: str, env: dict[str, str]) -> Path | None:
    _, _, rust_target = TARGETS[target]
    path = _cargo_target_dir(root, env) / rust_target / "release" / ("symbrowse.exe" if target.startswith("windows-") else "symbrowse")
    target_is_installed = subprocess.run(
        ["rustup", "target", "list", "--installed"], cwd=root, env=env, text=True, capture_output=True, check=False
    )
    if rust_target not in target_is_installed.stdout.splitlines():
        print(f"BLOCKED: Rust target {rust_target} is not installed", file=sys.stderr)
        return None
    build_env = dict(env, SYMBROWSE_VERSION=version.removeprefix("v"))
    command = ["cargo", "build", "--release", "-p", "symbrowse-cli", "--bin", "symbrowse", "--target", rust_target, "--locked"]
    return path if try_run(command, cwd=root, env=build_env) and path.is_file() else None


def archive_bytes(binary: Path, *, binary_name: str, root: Path, windows: bool) -> bytes:
    files: list[tuple[str, bytes, int]] = [(binary_name, binary.read_bytes(), 0o755)]
    for name in ("LICENSE", "README.md", "AGENTS.md"):
        source = root / name
        if source.is_file():
            files.append((name, source.read_bytes(), 0o644))
    if windows:
        from io import BytesIO

        output = BytesIO()
        with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as stream:
            for name, data, mode in files:
                info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = mode << 16
                stream.writestr(info, data)
        return output.getvalue()

    from io import BytesIO

    output = BytesIO()
    with gzip.GzipFile(fileobj=output, mode="wb", mtime=0) as compressed:
        with tarfile.open(fileobj=compressed, mode="w") as stream:
            for name, data, mode in files:
                info = tarfile.TarInfo(name)
                info.size = len(data)
                info.mode = mode
                info.uid = 0
                info.gid = 0
                info.uname = "root"
                info.gname = "root"
                info.mtime = 0
                stream.addfile(info, BytesIO(data))
    return output.getvalue()


def write_spdx(path: Path, archive_name: str, implementation: str, version: str) -> None:
    document = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"symbrowse-{implementation}-{archive_name}",
        "documentNamespace": f"https://spdx.symaira.dev/symbrowse/{implementation}/{archive_name}",
        "creationInfo": {"created": "1970-01-01T00:00:00Z", "creators": ["Tool: symaira-browse dual release builder"]},
        "packages": [{"SPDXID": "SPDXRef-Package-symbrowse", "name": "symbrowse", "versionInfo": version}],
    }
    path.write_text(json.dumps(document, sort_keys=True, indent=2) + "\n", encoding="utf-8")


def package(binary: Path, directory: Path, target: str, implementation: str, version: str, root: Path) -> dict[str, str]:
    os_name, arch, _ = TARGETS[target]
    binary_name = "symbrowse.exe" if os_name == "windows" else "symbrowse"
    extension = "zip" if os_name == "windows" else "tar.gz"
    archive_name = f"symbrowse_{version.removeprefix('v')}_{os_name}_{arch}.{extension}"
    archive = directory / archive_name
    archive.write_bytes(archive_bytes(binary, binary_name=binary_name, root=root, windows=os_name == "windows"))
    sbom = directory / f"{archive_name}.sbom"
    write_spdx(sbom, archive_name, implementation, version.removeprefix("v"))
    return {"archive": archive_name, "archive_sha256": sha256(archive), "sbom": sbom.name, "sbom_sha256": sha256(sbom)}


def write_manifests(output: Path, version: str, artifacts: dict[str, list[dict[str, str]]], proofs: list[dict[str, Any]]) -> None:
    dual = output / "dual"
    for implementation, entries in artifacts.items():
        directory = dual / implementation
        lines = [f"{entry['archive_sha256']}  {entry['archive']}" for entry in entries]
        lines += [f"{entry['sbom_sha256']}  {entry['sbom']}" for entry in entries]
        (directory / "checksums.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")
        (directory / "signature-inputs.json").write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "implementation": implementation,
                    "signing": "required",
                    "signed": False,
                    "artifacts": [
                        {
                            "artifact": entry["archive"],
                            "sha256": entry["archive_sha256"],
                            "signature": f"{entry['archive']}.sig",
                            "certificate": f"{entry['archive']}.pem",
                        }
                        for entry in entries
                    ],
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
    (dual / "platform-proofs.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": version.removeprefix("v"),
                "proofs": proofs,
                "runtime": "cross-built targets require native runner proof; Windows proof is external unless a Windows runner produced it",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    (dual / "dual-release-manifest.json").write_text(
        json.dumps(
            {
                "schema_version": 1,
                "layout": "dual/{go,rust}/<goreleaser-archive>",
                "default_implementation": "go",
                "release_state": "go-default; rust-opt-in",
                "signing": "required; no local signatures fabricated",
                "selection_contract": {
                    "default": "go",
                    "opt_in": "rust",
                    "forced_go": "go",
                    "availability_failure": "fallback_go",
                    "integrity_failure": "block_no_fallback",
                },
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", default=os.environ.get("GITHUB_REF_NAME", "0.0.0"))
    parser.add_argument("--output", type=Path, default=Path("target/release-evidence/dual"))
    parser.add_argument("--target", action="append", choices=tuple(TARGETS))
    parser.add_argument("--all-targets", action="store_true")
    parser.add_argument("--source-revision", required=True, help="integrated source commit SHA for candidate evidence")
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--go-binary", type=Path)
    parser.add_argument("--rust-binary", type=Path)
    parser.add_argument("--strict", action="store_true", help="fail if any selected implementation/target cannot be built")
    args = parser.parse_args(argv)
    version = args.version.removeprefix("v")
    targets = list(TARGETS) if args.all_targets else (args.target or [host_target()])
    root = ROOT
    env = dict(os.environ, CGO_ENABLED="0", GOTOOLCHAIN=os.environ.get("GOTOOLCHAIN", "go1.26.6"))
    external_environment(env)
    output = release_output(root, args.output, env)
    if output.exists():
        raise RuntimeError(f"refusing to overwrite existing release output: {output}")
    (output / "dual").mkdir(parents=True)
    artifacts: dict[str, list[dict[str, str]]] = {"go": [], "rust": []}
    for implementation in artifacts:
        (output / "dual" / implementation).mkdir(parents=True, exist_ok=True)
    proofs: list[dict[str, Any]] = []
    blocked: list[str] = []
    host = host_target()
    with tempfile.TemporaryDirectory(prefix="rust016-dual-", dir=env.get("TMPDIR")) as raw:
        staging = Path(raw)
        for target in targets:
            binaries: dict[str, Path | None] = {}
            if args.skip_build:
                if target != host or args.go_binary is None or args.rust_binary is None:
                    blocked.append(f"{target}: --skip-build requires host target and both binary paths")
                    continue
                binaries = {"go": args.go_binary.resolve(), "rust": args.rust_binary.resolve()}
            else:
                binaries = {
                    "go": go_binary(root, staging, target, version, env),
                    "rust": rust_binary(root, staging, target, version, env),
                }
            for implementation, binary in binaries.items():
                if binary is None or not binary.is_file():
                    blocked.append(f"{implementation}/{target}: build unavailable")
                    continue
                identity: dict[str, object] | None = None
                if target == host:
                    try:
                        identity = check_binary(binary, version)
                    except (OSError, ValueError, subprocess.SubprocessError) as error:
                        blocked.append(f"{implementation}/{target}: native version smoke failed: {error}")
                        continue
                entry = package(binary, output / "dual" / implementation, target, implementation, version, root)
                artifacts[implementation].append(entry)
                proofs.append(
                    {
                        "implementation": implementation,
                        "target": target,
                        "archive": entry["archive"],
                        "archive_sha256": entry["archive_sha256"],
                        "mode": "native" if target == host else "cross",
                        "status": "native_verified" if target == host else "cross_built",
                        "native_runtime_proof": target == host,
                        "version_smoke": identity is not None,
                        "schema_version": identity["schema_version"] if identity else None,
                        "source_revision": args.source_revision,
                        "runner": f"{platform.system()}/{platform.machine()}",
                    }
                )
    write_manifests(output, version, artifacts, proofs)
    report = {
        "schema_version": 1,
        "version": version,
        "source_revision": args.source_revision,
        "targets_requested": targets,
        "artifacts": {name: len(entries) for name, entries in artifacts.items()},
        "blocked": blocked,
        "signing": "not performed; signature-input manifests require a later signing job",
        "native_runtime_proof": sorted({proof["target"] for proof in proofs if proof["native_runtime_proof"]}),
        "cutover": "blocked until signatures, all six targets, native platform proofs and value gates pass",
    }
    (output / "build-report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 1 if args.strict and blocked else 0


if __name__ == "__main__":
    raise SystemExit(main())
