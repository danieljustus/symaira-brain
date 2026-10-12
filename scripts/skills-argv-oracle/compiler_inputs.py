"""Compiler-selected inputs for the existing isolated Skills Go build.

Uses the repository's go-list closure approach (Browse's validator), scoped to
./cmd/symbrain. The complete SDK is retained too; a receipt still needs an
independent review anchor before the native producer can admit it.
"""
import hashlib
import json
from pathlib import Path
import stat

from skills_reference import TOOLCHAIN


FIELDS = ("GoFiles", "CgoFiles", "CompiledGoFiles", "SFiles", "SysoFiles",
          "CFiles", "HFiles", "CXXFiles", "MFiles", "FFiles", "EmbedFiles")


def documents(raw):
    remaining = raw.decode("utf-8")
    decoder = json.JSONDecoder()
    rows = []
    while remaining.strip():
        row, end = decoder.raw_decode(remaining.lstrip())
        if not isinstance(row, dict) or row.get("Error") or row.get("DepsErrors"):
            raise ValueError("invalid compiler input description")
        rows.append(row)
        remaining = remaining.lstrip()[end:]
    if not rows:
        raise ValueError("empty compiler input description")
    return rows


def file_row(path, roots):
    if path.is_symlink() or not stat.S_ISREG(path.lstat().st_mode):
        raise ValueError("compiler input must be a regular file")
    resolved = path.resolve(strict=True)
    for scope, root in roots.items():
        if resolved.is_relative_to(root):
            raw = resolved.read_bytes()
            return {"scope": scope, "path": resolved.relative_to(root).as_posix(),
                    "bytes": len(raw), "sha256": hashlib.sha256(raw).hexdigest()}
    raise ValueError("compiler input escapes the owned source/module/SDK roots")


def collect(go, source, env, report, run):
    settings = json.loads(run([go, "env", "-json", "GOROOT", "GOVERSION", "GOOS", "GOARCH",
                               "GOHOSTOS", "GOHOSTARCH", "GOMOD", "GOMODCACHE", "CGO_ENABLED",
                               "GOWORK", "GOFLAGS", "GOEXPERIMENT"], env, report, source))
    if (settings["GOVERSION"] != TOOLCHAIN or settings["CGO_ENABLED"] != "0"
            or settings["GOOS"] != settings["GOHOSTOS"]
            or settings["GOARCH"] != settings["GOHOSTARCH"]
            or settings["GOFLAGS"] or settings["GOEXPERIMENT"]
            or settings["GOWORK"] != "off"
            or Path(settings["GOMOD"]).resolve() != source / "go.mod"
            or Path(settings["GOMODCACHE"]).resolve() != Path(env["GOMODCACHE"]).resolve()):
        raise ValueError("corrected compiler must be the pinned native unmodified SDK environment")
    sdk = Path(settings["GOROOT"]).resolve(strict=True)
    if Path(go).resolve(strict=True) != (sdk / "bin" / Path(go).name).resolve(strict=True):
        raise ValueError("Go executable is not from the selected SDK")
    # Downloaded toolchains live inside GOMODCACHE; classify the narrower SDK
    # first so its compiler-selected files cannot be mislabeled as dependencies.
    roots = {"source": source, "sdk": sdk, "modules": Path(env["GOMODCACHE"]).resolve()}
    packages = documents(run([go, "list", "-mod=readonly", "-deps", "-json", "./cmd/symbrain"],
                             env, report, source, timeout=300))
    modules = documents(run([go, "list", "-mod=readonly", "-m", "-json", "all"],
                            env, report, source, timeout=300))
    paths = {source / "go.mod", source / "go.sum"}
    for package in packages:
        if not isinstance(package.get("Dir"), str):
            raise ValueError("compiler package directory missing")
        for field in FIELDS:
            for name in package.get(field, []):
                if not isinstance(name, str):
                    raise ValueError("compiler input filename missing")
                paths.add(Path(package["Dir"]) / name)
    for module in modules:
        if module.get("Replace"):
            raise ValueError("corrected reference does not permit replacement modules")
        if module.get("GoMod"):
            paths.add(Path(module["GoMod"]))
    files = sorted((file_row(path, roots) for path in paths), key=lambda row: (row["scope"], row["path"]))
    if {row["scope"] for row in files} != set(roots):
        raise ValueError("incomplete compiler input closure")
    # Include the actual tool binaries and all SDK support files, not just the
    # selected standard-library sources or self-reported version string.
    sdk_files = []
    for path in sorted(sdk.rglob("*")):
        if path.is_symlink():
            raise ValueError("SDK symlinks are not admitted")
        if path.is_file():
            sdk_files.append(file_row(path, {"sdk": sdk}))
    if not sdk_files:
        raise ValueError("empty SDK closure")
    return {"go_env": settings, "files": files, "sdk_files": sdk_files, "modules": modules}
