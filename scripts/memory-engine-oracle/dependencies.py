"""Verify actual offline dependency bodies; never trust a borrowed cache path."""
import base64
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tomllib
import zipfile
import stat
import reader

HERE = Path(__file__).resolve().parent
COREKIT_REF = "04d1411adb57aa602b992509121011aa7666ff1a"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def hash1(files):
    result = hashlib.sha256()
    for name, data in sorted(files.items()):
        if "\n" in name:
            raise ValueError("newline in Go module hash name")
        result.update((hashlib.sha256(data).hexdigest() + "  " + name + "\n").encode())
    return "h1:" + base64.b64encode(result.digest()).decode()


def escaped(value):
    return "".join("!" + c.lower() if c.isupper() else c for c in value)


def go_inputs(source, cache, metadata):
    selected = {}
    sums = {}
    for line in (source / "go.sum").read_text().splitlines():
        module, version, checksum = line.split()
        base = "cache/download/" + escaped(module) + "/@v/" + escaped(version.removesuffix("/go.mod"))
        if version.endswith("/go.mod"):
            name = base + ".mod"
            if hash1({"go.mod": (cache / name).read_bytes()}) != checksum:
                raise ValueError("unbound Go module declaration: " + name)
            selected[name] = sha(cache / name)
        else:
            name = base + ".zip"
            with zipfile.ZipFile(cache / name) as archive:
                rows = archive.infolist()
                names = [row.filename for row in rows]
                if len(set(names)) != len(names):
                    raise ValueError("duplicate Go archive members")
                prefix = module + "@" + version + "/"
                files = {}
                for row in rows:
                    relative = PurePosixPath(row.filename)
                    if row.is_dir() or stat.S_ISLNK(row.external_attr >> 16) or not row.filename.startswith(prefix) or ".." in relative.parts or relative.is_absolute():
                        raise ValueError("invalid Go archive member: " + row.filename)
                    files[row.filename] = archive.read(row)
                if hash1(files) != checksum:
                    raise ValueError("unbound Go module source archive: " + name)
            selected[name] = sha(cache / name)
            sums[base + ".ziphash"] = checksum
    for name, expected in metadata.items():
        if sha(cache / name) != expected:
            raise ValueError("unbound Go offline metadata: " + name)
        selected[name] = expected
    return {"files": selected, "generated_ziphash": sums}


def cargo_inputs(source, cache, pinned):
    selected = dict(pinned["cargo_metadata"])
    lock = tomllib.loads((source / "Cargo.lock").read_text())
    git_sources = set()
    for package in lock["package"]:
        owner = package.get("source", "")
        if owner.startswith("registry+"):
            if owner != "registry+https://github.com/rust-lang/crates.io-index":
                raise ValueError("unreviewed Cargo registry owner")
            name = "registry/cache/" + pinned["registry"] + "/" + package["name"] + "-" + package["version"] + ".crate"
            if sha(cache / name) != package["checksum"]:
                raise ValueError("unbound Cargo source archive: " + name)
            # Cargo extracts this verified archive itself in a fresh private
            # home; never admit a possibly modified extracted registry tree.
            with tarfile.open(cache / name, "r:gz") as archive:
                prefix = package["name"] + "-" + package["version"]
                names = set()
                for row in archive:
                    path = PurePosixPath(row.name)
                    if row.name in names or path.is_absolute() or ".." in path.parts or not path.parts or path.parts[0] != prefix or not (row.isfile() or row.isdir()):
                        raise ValueError("unsafe Cargo source archive member")
                    names.add(row.name)
            selected[name] = package["checksum"]
        elif owner.startswith("git+"):
            git_sources.add(owner)
        elif owner:
            raise ValueError("unreviewed Cargo dependency owner")
    if git_sources != set(pinned["git_owners"]):
        raise ValueError("unreviewed Cargo Git source revision")
    selected.update(pinned["cargo_git_database"])
    for name, expected in selected.items():
        if (cache / name).is_symlink() or sha(cache / name) != expected:
            raise ValueError("unbound Cargo dependency input: " + name)
    if pinned["git_owners"]:
        actual = reader.blob_map(cache / pinned["git_database_dir"], pinned["git_revision"])
        if actual != pinned["git_source_blobs"]:
            raise ValueError("Cargo Git dependency source bodies differ from fixed revision")
    return {"files": selected}


def admit(go_source, rust_source, go_cache, cargo_cache):
    pinned = json.loads((HERE / "dependency-inputs.json").read_bytes())
    if pinned["git_revision"] != COREKIT_REF or any(not name.endswith("#" + COREKIT_REF) for name in pinned["git_owners"]):
        raise ValueError("dependency Git identity differs from fixed source owner")
    if sha(rust_source / "Cargo.lock") != pinned["cargo_lock_sha256"] or sha(go_source / "go.sum") != pinned["go_sum_sha256"]:
        raise ValueError("dependency declarations differ from reviewed owners")
    # A fresh home is populated only from these exact archive/index/Git bytes.
    # Missing locked archives are an admission failure, not permission to
    # borrow a registry tree, fetch online, or proceed with partial ownership.
    return {"go": go_inputs(go_source, go_cache, pinned["go_metadata"]),
            "rust": cargo_inputs(rust_source, cargo_cache, pinned)}


def materialize(root, selected, destination):
    destination.mkdir(parents=True, exist_ok=False)
    for name, expected in selected["files"].items():
        original, output = root / name, destination / name
        if original.is_symlink() or sha(original) != expected:
            raise ValueError("dependency changed before staging: " + name)
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(original, output)
        if sha(output) != expected:
            raise ValueError("staged dependency changed: " + name)
    for name, checksum in selected.get("generated_ziphash", {}).items():
        (destination / name).write_text(checksum)


def staged(destination, selected):
    for name in ("config", "config.toml"):
        if (destination / name).exists() or (destination / name).is_symlink():
            raise ValueError("unbound private dependency configuration")
    expected = set(selected["files"]) | set(selected.get("generated_ziphash", {}))
    for name, digest in selected["files"].items():
        if (destination / name).is_symlink() or sha(destination / name) != digest:
            raise ValueError("staged dependency bytes changed: " + name)
    for name, value in selected.get("generated_ziphash", {}).items():
        if (destination / name).read_text() != value:
            raise ValueError("generated Go archive hash changed")
    # The SDK creates extracted source and lock files while compiling. Thus
    # this verifies immutable archive/index inputs, not a whole mutable cache.
    return {"verified_inputs": len(expected)}
