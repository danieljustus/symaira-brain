"""Independent source/input/toolchain admission shared by build and replay."""
import hashlib
import json
from pathlib import Path
import platform
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
GO_REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
HELPER = "internal/memory/contextassembler/zz_native_engine_oracle_test.go"
PROOF_FILES = {"scripts/memory-engine-oracle/" + name for name in (
    "binding.py", "build.py", "replay.py", "trusted.json", "cases.json", "oracle_test.go.txt")}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def git(checkout, *arguments):
    return subprocess.check_output(["git", *arguments], cwd=checkout, text=True).strip()


def trusted():
    value = json.loads((HERE / "trusted.json").read_bytes())
    if value["frozen_go_commit"] != GO_REF:
        raise ValueError("trusted frozen Go identity changed")
    return value


def cases():
    plan = trusted()
    path = HERE / "cases.json"
    value = json.loads(path.read_bytes())
    identities = [[v["id"], v["operation"]] for v in value]
    if digest(path) != plan["cases_sha256"] or identities != plan["case_identities"] or len(value) != 60:
        raise ValueError("complete immutable60 case bytes/identities/order required")
    return value


def source_map(checkout, role):
    names = git(checkout, "ls-tree", "-r", "--name-only", "HEAD").splitlines()
    selected = [p for p in names if
                (role == "go" and (p.endswith(".go") or p.endswith("/go.mod") or
                                    p.endswith("/go.sum") or p in ("go.mod", "go.sum"))) or
                (role == "rust" and (p.endswith(".rs") or p.endswith("Cargo.toml") or
                                      p == "Cargo.lock" or p in PROOF_FILES))]
    requests = "".join("HEAD:" + name + "\n" for name in selected).encode()
    packed = subprocess.check_output(["git", "cat-file", "--batch"], cwd=checkout, input=requests)
    position, result = 0, {}
    for name in selected:
        end = packed.index(b"\n", position)
        header = packed[position:end].split()
        if len(header) != 3 or header[1] != b"blob":
            raise ValueError("missing tracked source blob: " + name)
        size = int(header[2])
        position = end + 1
        blob = packed[position:position + size]
        position += size
        if packed[position:position + 1] != b"\n":
            raise ValueError("invalid Git source boundary")
        position += 1
        expected = hashlib.sha256(blob).hexdigest()
        if digest(checkout / name) != expected:
            raise ValueError("modified tracked source: " + name)
        result[name] = expected
    if position != len(packed):
        raise ValueError("unexpected Git source records")
    return result


def sources(go_source, rust_source):
    plan = trusted()
    if git(go_source, "rev-parse", "HEAD") != GO_REF:
        raise ValueError("oracle is not immutable dcddcef0")
    if rust_source.resolve() != ROOT.resolve():
        raise ValueError("native source must own this reviewed runner")
    if git(rust_source, "status", "--porcelain", "--untracked-files=no"):
        raise ValueError("native checkout changed")
    original = source_map(go_source, "go")
    if original != plan["frozen_go_sources"]:
        raise ValueError("oracle source/dependency map differs from independent frozen map")
    if git(go_source, "diff", "--name-only") or git(go_source, "diff", "--cached", "--name-only"):
        raise ValueError("oracle tracked checkout changed")
    actual_go = {str(p.relative_to(go_source)) for p in go_source.rglob("*.go") if ".git" not in p.parts}
    if actual_go != {p for p in original if p.endswith(".go")} | {HELPER}:
        raise ValueError("unexpected or missing Go compilation source")
    if digest(go_source / HELPER) != plan["go_helper_sha256"]:
        raise ValueError("materialized helper differs from immutable original helper")
    if digest(HERE / "oracle_test.go.txt") != plan["go_helper_sha256"]:
        raise ValueError("original Go helper changed")
    return {"go": original, "rust": source_map(rust_source, "rust"),
            "go_commit": GO_REF, "rust_commit": git(rust_source, "rev-parse", "HEAD")}


def sdks(go_sdk, rust_sdk):
    key = platform.system().lower() + "-" + platform.machine().lower()
    plan = trusted()["sdk_platforms"].get(key)
    if plan is None:
        raise ValueError("native SDK platform not yet independently pinned: " + key)
    result = {}
    for role, root in (("go", go_sdk), ("rust", rust_sdk)):
        for name, expected in {**plan[role]["files"], **plan[role].get("source_files", {})}.items():
            if digest(root / name) != expected:
                raise ValueError("unbound SDK bytes: " + role + ":" + name)
        result[role] = plan[role]
    return result


def snapshot(go_source, rust_source, go_sdk, rust_sdk, binaries):
    return {"sources": sources(go_source, rust_source), "sdks": sdks(go_sdk, rust_sdk),
            "checkouts": {"go": str(go_source.resolve()), "rust": str(rust_source.resolve())},
            "SDK_roots": {"go": str(go_sdk.resolve()), "rust": str(rust_sdk.resolve())},
            "binary_paths": {role: str(path.resolve()) for role, path in binaries.items()},
            "cases_sha256": digest(HERE / "cases.json"),
            "trusted_sha256": digest(HERE / "trusted.json"),
            "binaries": {role: digest(path) for role, path in binaries.items()}}


def verify_receipt(receipt, observed, receipt_path):
    if receipt.get("kind") != "actual-memory760-build-v1" or receipt.get("snapshot") != observed:
        raise ValueError("actual source/SDK/input/executable build binding required")
    steps = receipt.get("steps", [])
    if len(steps) != 2 or [x.get("role") for x in steps] != ["go", "rust"]:
        raise ValueError("both actual builds required")
    expected = {
        "go": [str(Path(observed["SDK_roots"]["go"]) / "bin/go"), "test", "-p=2", "-mod=readonly", "-c", "-o", observed["binary_paths"]["go"], "./internal/memory/contextassembler"],
        "rust": [str(Path(observed["SDK_roots"]["rust"]) / "bin/cargo"), "build", "--locked", "--offline", "-p", "symbrain-memory", "--example", "engine_probe"],
    }
    for step in steps:
        role = step["role"]
        if step.get("argv") != expected[role] or str(Path(step["cwd"]).resolve()) != observed["checkouts"][role]:
            raise ValueError("actual build command/source owner changed")
        for stream in ("stdout", "stderr"):
            if digest(receipt_path.parent / (role + ".build." + stream)) != step.get(stream + "_sha256"):
                raise ValueError("actual build log changed")
        if step.get("exit") != 0 or not step.get("stdout_sha256") or not step.get("stderr_sha256"):
            raise ValueError("failed or incomplete actual build")
    if receipt.get("builder_sha256") != digest(HERE / "build.py"):
        raise ValueError("actual builder source changed")
