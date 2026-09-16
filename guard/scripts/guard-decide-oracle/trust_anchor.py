"""Load the review-controlled Rust trust anchor for the raw-byte corpus."""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
from pathlib import Path
import re


ANCHOR_PATH = (
    Path(__file__).resolve().parents[3]
    / "rust/symbrain-guard-core/tests/fixtures/external_repair_trust_anchor.rs"
)
_HASH_RE = re.compile(r"[0-9a-f]{64}")
_COMMIT_RE = re.compile(r"[0-9a-f]{40}")
_TOOLCHAIN_RE = re.compile(r"go[0-9]+\.[0-9]+\.[0-9]+")


@dataclass(frozen=True)
class TrustAnchor:
    """Literal identities reviewed outside the Python acceptance path."""

    oracle_commit: str
    go_toolchain: str
    fixture_sha256: str
    generator_sha256: str
    validator_sha256: str
    case_count: int
    source_files: tuple[tuple[str, str], ...]


def _single_scalar(source: str, name: str) -> str:
    matches = re.findall(
        rf"pub const {re.escape(name)}\s*:\s*&str\s*=\s*\"([^\"]+)\"\s*;",
        source,
    )
    if len(matches) != 1:
        raise AssertionError(f"trust anchor must define {name} exactly once")
    return matches[0]


def _single_case_count(source: str) -> int:
    matches = re.findall(
        r"pub const TRUSTED_CASE_COUNT\s*:\s*usize\s*=\s*([0-9]+)\s*;",
        source,
    )
    if len(matches) != 1:
        raise AssertionError("trust anchor must define TRUSTED_CASE_COUNT exactly once")
    return int(matches[0])


def _source_files(source: str) -> tuple[tuple[str, str], ...]:
    block_match = re.search(
        r"pub const TRUSTED_SOURCE_FILES\s*:\s*\[\s*\(\s*&str\s*,\s*&str\s*\)\s*;\s*3\s*\]\s*=\s*\[(?P<body>.*?)\];",
        source,
        re.DOTALL,
    )
    if block_match is None:
        raise AssertionError("trust anchor source-file block is missing")
    entries = tuple(
        re.findall(
            r"\(\s*\"([^\"]+)\"\s*,\s*\"([0-9a-f]{64})\"\s*,?\s*\)",
            block_match.group("body"),
        )
    )
    if len(entries) != 3 or len({path for path, _ in entries}) != len(entries):
        raise AssertionError("trust anchor source-file inventory is invalid")
    return entries


def load_anchor(path: Path = ANCHOR_PATH) -> TrustAnchor:
    """Parse and validate the Rust literals without importing generated code."""

    path = Path(path).resolve()
    if not path.is_file():
        raise AssertionError(f"trust anchor is missing: {path}")
    source = path.read_text(encoding="utf-8")
    oracle_commit = _single_scalar(source, "TRUSTED_ORACLE_COMMIT")
    go_toolchain = _single_scalar(source, "TRUSTED_GO_TOOLCHAIN")
    fixture_sha256 = _single_scalar(source, "TRUSTED_FIXTURE_SHA256")
    generator_sha256 = _single_scalar(source, "TRUSTED_GENERATOR_SHA256")
    validator_sha256 = _single_scalar(source, "TRUSTED_VALIDATOR_SHA256")
    if _COMMIT_RE.fullmatch(oracle_commit) is None:
        raise AssertionError("trust anchor oracle commit is not a full SHA")
    if _TOOLCHAIN_RE.fullmatch(go_toolchain) is None:
        raise AssertionError("trust anchor Go toolchain is invalid")
    for label, value in (
        ("fixture", fixture_sha256),
        ("generator", generator_sha256),
        ("validator", validator_sha256),
    ):
        if _HASH_RE.fullmatch(value) is None:
            raise AssertionError(f"trust anchor {label} digest is invalid")
    return TrustAnchor(
        oracle_commit=oracle_commit,
        go_toolchain=go_toolchain,
        fixture_sha256=fixture_sha256,
        generator_sha256=generator_sha256,
        validator_sha256=validator_sha256,
        case_count=_single_case_count(source),
        source_files=_source_files(source),
    )


def file_sha256(path: Path, *, normalize_crlf: bool = False) -> str:
    """Hash a file, optionally applying the repository's text normalization."""

    data = Path(path).read_bytes()
    if normalize_crlf:
        data = data.replace(b"\r\n", b"\n")
    return hashlib.sha256(data).hexdigest()
