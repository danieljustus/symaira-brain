"""Fixed Skills reference identities shared by the build and native producers."""
import hashlib
import io
import json
import platform
import re
import subprocess
import sys
import zipfile

if not __debug__:
    raise RuntimeError("Skills reference admission requires Python assertions enabled")

HISTORICAL = "historical"
CORRECTED = "skills-control-nofollow-v1"
REFERENCES = (HISTORICAL, CORRECTED)
FROZEN = "dcddcef0df5789123c7c9a7ebe6e01f10e941f2c"
TOOLCHAIN = "go1.26.7"
# Entire dcddcef0 tree plus the four reviewed Go files, including both tests.
# This is a source identity, NOT compiler or independent build evidence.
CORRECTED_TREE = "8161fed7fef8b3e48b318966c0693ebc76c585b942b1e04efece858d3bf6436f"
# Fixed reviewed source anchor, retained independently of candidate HEAD.
# No CLI/env override: a source anchor alone must never admit a product run.
CORRECTED_COMMIT = "ce909711d6e7125d281840d14744e7aa987f626f"
CORRECTED_ANCHOR = "refs/heads/oracle/skills-control-nofollow-v1"
# Independent reviews132 (Darwin) and147 (Linux/Windows) APPROVE/APPROVE.
# Build admission is NOT process parity. Never derive these pins from input.
CORRECTED_RECEIPTS = {
    ("darwin", "arm64"): "7aaacf61391f4edc0d7ad2f0084f9296d14527f500d8a3923bd569a2f585fb74",
    ("linux", "x86_64"): "02d809361504618991e8c76beed233767a0f883d42764001e9176524261e9ad6",
    ("win32", "AMD64"): "58392844cb3f90d6b9b95961daade3a90c43d72d43f9ad5b147783600fd9f4b8",
}
# Original API/ZIP identities verified by review147, not caller-selected URLs.
BUILD_HEAD = "578d201f579c45d13af3c076277e87ef8dcf6f74"
BUILD_BRANCH = "issue/764-json-eof-state-correction"
REPOSITORY = "danieljustus/symaira-brain"
REVIEWED_ARTIFACTS = {
    ("linux", "x86_64"): {
        "runner": "ubuntu-latest", "run": 38186442269, "job": 114609664447,
        "artifact": 11719806790, "bytes": 15248387, "binary": "symbrain",
        "zip_sha256": "fbcedc0a44425cebfcd8e5277aa3665290be76ac9838b43544f40fa90131dfc1",
        "binary_sha256": "2be5e25ada77c04b90b1cf78b653fc3ec28ba21824ff6124b9ea79e9e7277eba",
    },
    ("win32", "AMD64"): {
        "runner": "windows-latest", "run": 38186449761, "job": 114609687444,
        "artifact": 11719657439, "bytes": 15475522, "binary": "symbrain.exe",
        "zip_sha256": "2bb7156a1b7f8d88a1f1c35b293512e77aa011915052eeb8e58947e5604aaff8",
        "binary_sha256": "f1a2abb03711390980718c062c686697df8595d5830c9889d15b963f921cc86f",
    },
}


def source_commit(reference):
    if reference == HISTORICAL:
        return FROZEN
    if reference != CORRECTED:
        raise ValueError("unknown Skills reference")
    if not isinstance(CORRECTED_COMMIT, str) or not re.fullmatch(r"[0-9a-f]{40}", CORRECTED_COMMIT):
        raise ValueError("corrected reference awaits a durable reviewed source commit")
    if CORRECTED_COMMIT == FROZEN:
        raise ValueError("invalidated historical reference is not the corrected reference")
    if not isinstance(CORRECTED_ANCHOR, str) or not re.fullmatch(r"refs/(heads|tags)/[A-Za-z0-9_/-]+", CORRECTED_ANCHOR):
        raise ValueError("corrected reference awaits a durable reviewed source anchor")
    return CORRECTED_COMMIT


def tree_identity(rows):
    tree = {bytes.fromhex(row["path_bytes_hex"]).decode():
            {"mode": row["git_mode"], "blob": row["blob"]} for row in rows}
    if len(tree) != len(rows):
        raise ValueError("duplicate source paths")
    return hashlib.sha256(json.dumps(tree, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def require_source(rows, reference):
    source_commit(reference)
    if reference == CORRECTED and tree_identity(rows) != CORRECTED_TREE:
        raise ValueError("corrected reference is not the complete reviewed source tree")


def startup_identity(reference, receipt_sha256=None):
    commit = source_commit(reference)
    if reference == HISTORICAL:
        return {"frozen_go": commit}
    expected = CORRECTED_RECEIPTS.get((sys.platform, platform.machine()))
    if not isinstance(expected, str) or not re.fullmatch(r"[0-9a-f]{64}", expected):
        raise ValueError("corrected reference awaits independently anchored native build provenance")
    if receipt_sha256 != expected:
        raise ValueError("corrected reference receipt is not the independently reviewed native build")
    return {"frozen_go": commit, "reference": CORRECTED,
            "source_tree_sha256": CORRECTED_TREE, "oracle_receipt_sha256": expected}


def admit_receipt(reference, path, binary):
    # Resolve the approved identity BEFORE reading caller-controlled claims.
    commit = source_commit(reference)
    raw = path.read_bytes()
    receipt_sha256 = hashlib.sha256(raw).hexdigest()
    identity = startup_identity(reference, receipt_sha256)
    receipt = json.loads(raw)
    if receipt.get("status") != "passed" or receipt.get("frozen") != commit:
        raise ValueError("missing/invalidated/wrong Skills reference build")
    if receipt.get("reference", HISTORICAL) != reference:
        raise ValueError("wrong Skills reference label")
    if receipt.get("output_sha256") != hashlib.sha256(binary.read_bytes()).hexdigest():
        raise ValueError("Skills reference executable does not match build receipt")
    if reference == CORRECTED:
        if receipt.get("toolchain") != TOOLCHAIN:
            raise ValueError("wrong corrected reference SDK")
        require_source(receipt["source_before"], reference)
        if receipt["source_before"] != receipt["source_after"]:
            raise ValueError("corrected reference source changed during build")
        inputs = receipt.get("compiler_inputs_before")
        if not inputs or inputs != receipt.get("compiler_inputs_after"):
            raise ValueError("missing/changed corrected compiler inputs")
        if not all(inputs.get(field) for field in ("files", "sdk_files", "modules", "go_env")):
            raise ValueError("incomplete corrected compiler/module/SDK closure")
    return identity


def fetch_reviewed_reference(directory, runner):
    """Fetch ONLY the reviewed native binary/receipt; never build or execute it."""
    key = (sys.platform, platform.machine())
    approved = REVIEWED_ARTIFACTS.get(key)
    if approved is None or runner != approved["runner"]:
        raise ValueError("matching reviewed Linux/Windows native runner required")
    # Fail before network/files on absent or changed admission/source anchors.
    startup_identity(CORRECTED, CORRECTED_RECEIPTS.get(key))
    directory.mkdir()  # A prior attempt is never overwritten or cleaned up.
    base = f"repos/{REPOSITORY}/actions"

    def get(endpoint, name):
        result = subprocess.run(
            ["gh", "api", "--hostname", "github.com", "--method", "GET", endpoint],
            capture_output=True, timeout=120, check=False)
        (directory / name).write_bytes(result.stdout)
        (directory / (name + ".stderr")).write_bytes(result.stderr)
        (directory / (name + ".exit")).write_text(str(result.returncode) + "\n")
        result.check_returncode()
        return result.stdout

    def require(row, expected, label):
        if any(row.get(field) != value for field, value in expected.items()):
            raise ValueError(f"reviewed {label} identity mismatch")

    run = json.loads(get(f"{base}/runs/{approved['run']}", "run.json"))
    require(run, {"id": approved["run"], "run_attempt": 1, "head_sha": BUILD_HEAD,
                  "head_branch": BUILD_BRANCH, "event": "workflow_dispatch",
                  "path": ".github/workflows/skills-native.yml",
                  "status": "completed", "conclusion": "success"}, "run")
    require(run["repository"], {"full_name": REPOSITORY}, "repository")
    job = json.loads(get(f"{base}/jobs/{approved['job']}", "job.json"))
    require(job, {"id": approved["job"], "run_id": approved["run"], "run_attempt": 1,
                  "head_sha": BUILD_HEAD, "status": "completed", "conclusion": "success",
                  "name": f"Corrected Go BUILD ONLY ({runner})"}, "job")
    artifact_path = f"{base}/artifacts/{approved['artifact']}"
    artifact = json.loads(get(artifact_path, "artifact.json"))
    require(artifact, {"id": approved["artifact"], "size_in_bytes": approved["bytes"],
                       "name": f"{CORRECTED}-build-only-{runner}-{approved['run']}-1",
                       "digest": "sha256:" + approved["zip_sha256"],
                       "url": "https://api.github.com/" + artifact_path,
                       "archive_download_url": "https://api.github.com/" + artifact_path + "/zip"}, "artifact")
    if artifact.get("expired") is not False:
        raise ValueError("reviewed artifact expired or availability unknown")
    require(artifact["workflow_run"], {"id": approved["run"], "head_sha": BUILD_HEAD,
                                      "head_branch": BUILD_BRANCH, "repository_id": 1307519180,
                                      "head_repository_id": 1307519180}, "artifact run")
    raw = get(artifact_path + "/zip", "artifact.zip")
    if len(raw) != approved["bytes"] or hashlib.sha256(raw).hexdigest() != approved["zip_sha256"]:
        raise ValueError("reviewed artifact ZIP bytes mismatch")
    # Never extract caller-controlled paths or trust an unverified ZIP's hashes.
    with zipfile.ZipFile(io.BytesIO(raw)) as archive:
        names = {"go-build.json", approved["binary"], "build.stdout.log", "build.stderr.log"}
        if set(archive.namelist()) != names or len(archive.namelist()) != len(names):
            raise ValueError("reviewed artifact members mismatch")
        for name in sorted(names):
            with (directory / name).open("xb") as stream:
                stream.write(archive.read(name))
    binary = directory / approved["binary"]
    if hashlib.sha256(binary.read_bytes()).hexdigest() != approved["binary_sha256"]:
        raise ValueError("reviewed artifact binary bytes mismatch")
    identity = admit_receipt(CORRECTED, directory / "go-build.json", binary)
    # upload-artifact ZIPs do not retain executable mode. Only admitted bytes
    # receive it; chmod is not a binary rewrite or product invocation.
    binary.chmod(0o700)
    (directory / "admission.json").write_text(json.dumps(identity, indent=2) + "\n")
    return identity
