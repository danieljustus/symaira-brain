"""Fixed Skills reference identities shared by the build and native producers."""
import hashlib
import json
import platform
import re
import sys

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
# Local source anchor; remote publication remains pending.
# No CLI/env override: a source anchor alone must never admit a product run.
CORRECTED_COMMIT = "ce909711d6e7125d281840d14744e7aa987f626f"
CORRECTED_ANCHOR = "refs/heads/oracle/skills-control-nofollow-v1"
# Independent native review132 APPROVE/APPROVE; see the native-oracle README.
# No other platform has a reviewed receipt. Never derive these pins from input.
CORRECTED_RECEIPTS = {
    ("darwin", "arm64"): "7aaacf61391f4edc0d7ad2f0084f9296d14527f500d8a3923bd569a2f585fb74",
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
