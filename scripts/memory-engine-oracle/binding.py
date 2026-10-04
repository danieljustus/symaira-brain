"""Independent source/input/toolchain admission shared by build and replay."""
import hashlib
import json
from pathlib import Path
import platform
import reader
import owner
import dependencies
import sdk

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
GO_REF = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
HELPER = "internal/memory/contextassembler/zz_native_engine_oracle_test.go"
PROOF_FILES = {"scripts/memory-engine-oracle/" + name for name in (
    "binding.py", "build.py", "replay.py", "trusted.json", "cases.json", "oracle_test.go.txt",
    "owner.py", "dependencies.py", "reader.py", "dependency-inputs.json", "sdk.py", "sdk-inputs.json")}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def git(checkout, *arguments):
    return reader.query(checkout, *arguments, text=True).strip()


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
                role in ("rust", "owned")]
    requests = "".join("HEAD:" + name + "\n" for name in selected).encode()
    packed = reader.query(checkout, "cat-file", "--batch", input=requests)
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
    if git(rust_source, "status", "--porcelain", "--untracked-files=all"):
        raise ValueError("native checkout changed")
    if git(rust_source, "ls-files", "--others", "--ignored", "--exclude-standard"):
        raise ValueError("native checkout has ignored unbound inputs")
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
    native = source_map(rust_source, "rust")
    owned_go = source_map(go_source, "owned")
    owned_go[HELPER] = plan["go_helper_sha256"]
    owner.configurations(rust_source)
    owner.physical(rust_source, native, "rust")
    owner.physical(go_source, owned_go, "go")
    return {"go": original, "rust": native, "owned_go": owned_go,
            "go_commit": GO_REF, "rust_commit": git(rust_source, "rev-parse", "HEAD")}


def sdks(go_sdk, rust_sdk):
    key = platform.system().lower() + "-" + platform.machine().lower()
    plan = trusted()["sdk_platforms"].get(key)
    if plan is None:
        raise ValueError("native SDK platform not yet independently pinned: " + key)
    inventory_plan = json.loads((HERE / "sdk-inputs.json").read_bytes())
    if inventory_plan.get("kind") != "memory760-whole-sdk-v1" or key not in inventory_plan["platforms"]:
        raise ValueError("complete independently pinned SDK inventory required")
    result = {}
    for role, root in (("go", go_sdk), ("rust", rust_sdk)):
        for name, expected in {**plan[role]["files"], **plan[role].get("source_files", {})}.items():
            if digest(root / name) != expected:
                raise ValueError("unbound SDK bytes: " + role + ":" + name)
        observed = sdk.admit(root, role, inventory_plan["platforms"][key][role])
        result[role] = {**plan[role], "physical_inventory": observed}
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
    if receipt.get("kind") != "actual-memory760-build-v2" or receipt.get("snapshot") != observed:
        raise ValueError("actual source/SDK/input/executable build binding required")
    steps = receipt.get("steps", [])
    if len(steps) != 2 or [x.get("role") for x in steps] != ["go", "rust"]:
        raise ValueError("both actual builds required")
    expected = {
        "go": [str(Path(observed["SDK_roots"]["go"]) / "bin/go"), "test", "-p=2", "-mod=readonly", "-c", "-o", observed["binary_paths"]["go"], "./internal/memory/contextassembler"],
        "rust": [str(Path(observed["SDK_roots"]["rust"]) / "bin/cargo"), "build", "--locked", "--offline", "-p", "symbrain-memory", "--example", "engine_probe", "--message-format=json"],
    }
    for step in steps:
        role = step["role"]
        owned = Path(receipt["owned_checkouts"][role]).resolve()
        if step.get("argv") != expected[role] or str(Path(step["cwd"]).resolve()) != str(owned):
            raise ValueError("actual build command/source owner changed")
        names = observed["sources"]["owned_go" if role == "go" else "rust"]
        for name, expected_hash in names.items():
            if digest(owned / name) != expected_hash:
                raise ValueError("actual owned build source changed")
        owner.configurations(owned)
        owner.physical(owned, names, role)
        for stream in ("stdout", "stderr"):
            if digest(receipt_path.parent / (role + ".build." + stream)) != step.get(stream + "_sha256"):
                raise ValueError("actual build log changed")
        if step.get("exit") != 0 or not step.get("stdout_sha256") or not step.get("stderr_sha256"):
            raise ValueError("failed or incomplete actual build")
    if receipt.get("builder_sha256") != digest(HERE / "build.py"):
        raise ValueError("actual builder source changed")
    if (receipt_path.parent / "empty-git-config").read_bytes() != b"":
        raise ValueError("owned Git configuration changed")
    env = owner.environment(receipt_path.parent, Path(observed["SDK_roots"]["go"]),
                            Path(observed["SDK_roots"]["rust"]), Path(receipt["target"]))
    if receipt.get("environment") != env:
        raise ValueError("actual hermetic build environment changed")
    cache_roots = receipt["dependency_roots"]
    inputs = dependencies.admit(Path(observed["checkouts"]["go"]), Path(observed["checkouts"]["rust"]),
                                Path(cache_roots["go"]), Path(cache_roots["rust"]))
    if inputs != receipt.get("dependency_inputs"):
        raise ValueError("dependency source bodies changed")
    for role, folder in (("go", "go-modcache"), ("rust", "cargo-home")):
        dependencies.staged(receipt_path.parent / folder, inputs[role])
    artifact = owner.artifact((receipt_path.parent / "rust.build.stdout").read_bytes(),
                              Path(receipt["target"]), Path(receipt["owned_checkouts"]["rust"]) / "rust/symbrain-memory/Cargo.toml")
    if str(artifact) != observed["binary_paths"]["rust"]:
        raise ValueError("native executable is not actual Cargo artifact")
