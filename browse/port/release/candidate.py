#!/usr/bin/env python3
"""Merge and verify six native dual Go/Rust packages without signing or release."""
from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import sys
from pathlib import Path
from typing import Any

if str(Path(__file__).resolve().parent) not in sys.path:
    sys.path.insert(0, str(Path(__file__).resolve().parent))

import build_dual
import verify


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def read_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise verify.GateError(f"invalid candidate evidence {path}: {error}") from error
    if not isinstance(value, dict):
        raise verify.GateError(f"candidate evidence is not a JSON object: {path}")
    return value


def _runner_matches(proof: dict[str, Any], target: str) -> bool:
    expected_os, expected_arch, _ = build_dual.TARGETS[target]
    actual_os, actual_arch = str(proof.get("runner", "")).split("/", maxsplit=1)
    actual_os = {"Darwin": "darwin", "Linux": "linux", "Windows": "windows"}.get(actual_os, actual_os.lower())
    actual_arch = {"x86_64": "amd64", "AMD64": "amd64", "aarch64": "arm64", "ARM64": "arm64"}.get(
        actual_arch, actual_arch.lower()
    )
    return (actual_os, actual_arch) == (expected_os, expected_arch)


def _validate_target_package(root: Path, target: str, version: str, source_revision: str) -> list[dict[str, Any]]:
    report = read_json(root / "build-report.json")
    if (
        report.get("version") != version
        or report.get("source_revision") != source_revision
        or report.get("targets_requested") != [target]
        or report.get("blocked") != []
        or report.get("artifacts") != {"go": 1, "rust": 1}
        or report.get("native_runtime_proof") != [target]
    ):
        raise verify.GateError(f"native package build report is incomplete or mismatched for {target}")

    dual = root / "dual"
    evidence = read_json(dual / verify.PLATFORM_PROOFS_NAME)
    proofs = evidence.get("proofs")
    if evidence.get("version") != version or not isinstance(proofs, list) or len(proofs) != 2:
        raise verify.GateError(f"native platform evidence is incomplete for {target}")
    if not (dual / "dual-release-manifest.json").is_file():
        raise verify.GateError(f"missing dual release manifest for {target}")

    validated: list[dict[str, Any]] = []
    for implementation in verify.IMPLEMENTATIONS:
        directory = dual / implementation
        files = [item for item in directory.iterdir() if item.is_file()]
        expected_archive = verify.archive_name(version, *build_dual.TARGETS[target][:2])
        archive_path = directory / expected_archive
        sbom_path = directory / f"{expected_archive}.sbom"
        if set(path.name for path in files) != {expected_archive, sbom_path.name, "checksums.txt", verify.SIGNATURE_INPUTS_NAME}:
            raise verify.GateError(f"unexpected or missing unsigned candidate files for {implementation}/{target}")
        checksums = verify._read_checksum_manifest(directory / "checksums.txt")
        if checksums != {expected_archive: sha256(archive_path), sbom_path.name: sha256(sbom_path)}:
            raise verify.GateError(f"target checksum manifest mismatch for {implementation}/{target}")
        verify._verify_spdx(sbom_path)
        signature_inputs = read_json(directory / verify.SIGNATURE_INPUTS_NAME)
        entries = signature_inputs.get("artifacts")
        if (
            signature_inputs.get("schema_version") != 1
            or signature_inputs.get("implementation") != implementation
            or signature_inputs.get("signing") != "required"
            or signature_inputs.get("signed") is not False
            or not isinstance(entries, list)
            or entries != [{
                "artifact": expected_archive,
                "sha256": sha256(archive_path),
                "signature": f"{expected_archive}.sig",
                "certificate": f"{expected_archive}.pem",
            }]
        ):
            raise verify.GateError(f"signature inputs are not correctly unsigned for {implementation}/{target}")
        proof = next(
            (item for item in proofs if isinstance(item, dict) and item.get("implementation") == implementation), None
        )
        if (
            proof is None
            or proof.get("target") != target
            or proof.get("archive") != expected_archive
            or proof.get("archive_sha256") != sha256(archive_path)
            or proof.get("source_revision") != source_revision
            or proof.get("mode") != "native"
            or proof.get("status") != "native_verified"
            or proof.get("native_runtime_proof") is not True
            or proof.get("version_smoke") is not True
            or not isinstance(proof.get("schema_version"), int)
            or not _runner_matches(proof, target)
        ):
            raise verify.GateError(f"native runtime/version evidence mismatch for {implementation}/{target}")
        validated.append(proof)
    if validated[0]["schema_version"] != validated[1]["schema_version"]:
        raise verify.GateError(f"Go/Rust schema versions differ for {target}")
    return proofs


def merge(packages: Path, output: Path, version: str, source_revision: str) -> dict[str, Any]:
    if not re.fullmatch(r"[0-9a-f]{40,64}", source_revision):
        raise verify.GateError("source revision must be a full lowercase Git SHA")
    if output.exists():
        raise verify.GateError(f"refusing to overwrite existing candidate output: {output}")
    current = __import__("subprocess").run(
        ["git", "rev-parse", "HEAD"], cwd=build_dual.ROOT, capture_output=True, text=True, check=True
    ).stdout.strip()
    if current != source_revision:
        raise verify.GateError(f"checkout SHA {current} does not match candidate source revision {source_revision}")

    targets = [f"{os_name}-{arch}" for os_name, arch in verify.TARGETS]
    roots = {target: packages / f"symbrowse-dual-{source_revision}-{target}" for target in targets}
    expected_dirs = {path.name for path in roots.values()}
    if not packages.is_dir() or {path.name for path in packages.iterdir() if path.is_dir()} != expected_dirs:
        raise verify.GateError("downloaded package directories do not contain exactly the six same-SHA target artifacts")
    proofs: list[dict[str, Any]] = []
    for target, root in roots.items():
        proofs.extend(_validate_target_package(root, target, version, source_revision))

    dual = output / "dual"
    for implementation in verify.IMPLEMENTATIONS:
        (dual / implementation).mkdir(parents=True)
    artifacts: dict[str, list[dict[str, str]]] = {name: [] for name in verify.IMPLEMENTATIONS}
    for target, root in roots.items():
        for implementation in verify.IMPLEMENTATIONS:
            source = root / "dual" / implementation
            destination = dual / implementation
            archive = next(path for path in source.iterdir() if path.name.endswith((".zip", ".tar.gz")))
            sbom = source / f"{archive.name}.sbom"
            shutil.copyfile(archive, destination / archive.name)
            shutil.copyfile(sbom, destination / sbom.name)
            artifacts[implementation].append(
                {"archive": archive.name, "archive_sha256": sha256(archive), "sbom": sbom.name, "sbom_sha256": sha256(sbom)}
            )

    for implementation, entries in artifacts.items():
        directory = dual / implementation
        (directory / "checksums.txt").write_text(
            "\n".join(
                line
                for entry in entries
                for line in (f"{entry['archive_sha256']}  {entry['archive']}", f"{entry['sbom_sha256']}  {entry['sbom']}")
            )
            + "\n",
            encoding="utf-8",
        )
        (directory / verify.SIGNATURE_INPUTS_NAME).write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "implementation": implementation,
                    "signing": "required",
                    "signed": False,
                    "artifacts": [
                        {
                            "artifact": item["archive"],
                            "sha256": item["archive_sha256"],
                            "signature": f"{item['archive']}.sig",
                            "certificate": f"{item['archive']}.pem",
                        }
                        for item in entries
                    ],
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )
    shutil.copyfile(roots[targets[0]] / "dual" / "dual-release-manifest.json", dual / "dual-release-manifest.json")
    (dual / verify.PLATFORM_PROOFS_NAME).write_text(
        json.dumps(
            {
                "schema_version": 1,
                "version": version,
                "source_revision": source_revision,
                "proofs": proofs,
                "signing": "not performed; this is unsigned CI candidate evidence only",
                "publication": "not performed",
                "cutover": "not enabled",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    evidence = check(dual, version, source_revision)
    (output / "candidate-evidence.json").write_text(json.dumps(evidence, indent=2) + "\n", encoding="utf-8")
    return evidence


def check(candidate: Path, version: str, source_revision: str) -> dict[str, Any]:
    dual = candidate / "dual" if (candidate / "dual").is_dir() else candidate
    if not re.fullmatch(r"[0-9a-f]{40,64}", source_revision):
        raise verify.GateError("source revision must be a full lowercase Git SHA")
    implementation_report: dict[str, int] = {}
    specs: dict[str, list[verify.ArchiveSpec]] = {}
    for implementation in verify.IMPLEMENTATIONS:
        directory = dual / implementation
        expected = {verify.archive_name(version, os_name, arch) for os_name, arch in verify.TARGETS}
        expected_files = expected | {f"{name}.sbom" for name in expected} | {"checksums.txt", verify.SIGNATURE_INPUTS_NAME}
        actual_files = {path.name for path in directory.iterdir() if path.is_file()}
        if actual_files != expected_files:
            raise verify.GateError(f"{implementation} candidate has unexpected or missing package files")
        archives = {path.name: path for path in directory.iterdir() if path.is_file() and path.name.endswith((".zip", ".tar.gz"))}
        if set(archives) != expected:
            raise verify.GateError(f"{implementation} candidate does not have the exact six archive names")
        sums = verify._read_checksum_manifest(directory / "checksums.txt")
        if set(sums) != expected | {f"{name}.sbom" for name in expected}:
            raise verify.GateError(f"{implementation} candidate checksum matrix is incomplete")
        signature_inputs = read_json(directory / verify.SIGNATURE_INPUTS_NAME)
        items = signature_inputs.get("artifacts")
        if (
            signature_inputs.get("schema_version") != 1
            or signature_inputs.get("implementation") != implementation
            or signature_inputs.get("signing") != "required"
            or signature_inputs.get("signed") is not False
            or not isinstance(items, list)
            or len(items) != len(expected)
        ):
            raise verify.GateError(f"{implementation} signature input manifest is not unsigned and complete")
        seen: set[str] = set()
        validated: list[verify.ArchiveSpec] = []
        for os_name, arch in verify.TARGETS:
            name = verify.archive_name(version, os_name, arch)
            path = archives[name]
            spec = verify.ArchiveSpec(implementation, version, os_name, arch, path)
            members, modes = verify._read_archive_members(path)
            if spec.binary_name not in members or (os_name != "windows" and not (modes.get(spec.binary_name, 0) & 0o100)):
                raise verify.GateError(f"{name} lacks an executable {spec.binary_name}")
            sbom = directory / f"{name}.sbom"
            verify._verify_spdx(sbom)
            sbom_document = read_json(sbom)
            package_entries = sbom_document.get("packages")
            if (
                not isinstance(package_entries, list)
                or len(package_entries) != 1
                or not isinstance(package_entries[0], dict)
                or package_entries[0].get("name") != "symbrowse"
                or package_entries[0].get("versionInfo") != version
            ):
                raise verify.GateError(f"SBOM package identity mismatch for {name}")
            if sums[name] != sha256(path) or sums[sbom.name] != sha256(sbom):
                raise verify.GateError(f"checksum mismatch for {name} or its SBOM")
            manifest_entry = next((item for item in items if isinstance(item, dict) and item.get("artifact") == name), None)
            if (
                manifest_entry is None
                or manifest_entry.get("sha256") != sha256(path)
                or manifest_entry.get("signature") != f"{name}.sig"
                or manifest_entry.get("certificate") != f"{name}.pem"
            ):
                raise verify.GateError(f"unsigned signature-input digest mismatch for {name}")
            if (directory / f"{name}.sig").exists() or (directory / f"{name}.pem").exists():
                raise verify.GateError(f"unsigned candidate unexpectedly contains signature material for {name}")
            seen.add(name)
            validated.append(spec)
        if seen != {item.get("artifact") for item in items if isinstance(item, dict)}:
            raise verify.GateError(f"{implementation} signature inputs include unexpected artifacts")
        implementation_report[implementation] = len(validated)
        specs[implementation] = validated

    proof_doc = read_json(dual / verify.PLATFORM_PROOFS_NAME)
    proofs = proof_doc.get("proofs")
    expected_proofs = {(impl, f"{os_name}-{arch}") for impl in verify.IMPLEMENTATIONS for os_name, arch in verify.TARGETS}
    if (
        proof_doc.get("schema_version") != 1
        or proof_doc.get("version") != version
        or proof_doc.get("source_revision") != source_revision
        or proof_doc.get("publication") != "not performed"
        or proof_doc.get("cutover") != "not enabled"
        or not isinstance(proofs, list)
        or len(proofs) != len(expected_proofs)
    ):
        raise verify.GateError("merged native proof identity or matrix is incomplete")
    seen_proofs: set[tuple[str, str]] = set()
    schema_versions: dict[str, int] = {}
    for proof in proofs:
        if not isinstance(proof, dict):
            raise verify.GateError("invalid merged native proof")
        key = (str(proof.get("implementation")), str(proof.get("target")))
        implementation, target = key
        if key not in expected_proofs or key in seen_proofs or proof.get("source_revision") != source_revision:
            raise verify.GateError(f"unexpected or duplicate native proof: {key}")
        if (
            proof.get("status") != "native_verified"
            or proof.get("mode") != "native"
            or proof.get("native_runtime_proof") is not True
            or proof.get("version_smoke") is not True
            or not _runner_matches(proof, target)
        ):
            raise verify.GateError(f"native execution or runner identity is missing for {key}")
        schema = proof.get("schema_version")
        if not isinstance(schema, int) or schema < 1:
            raise verify.GateError(f"invalid CLI schema version for {key}")
        schema_versions.setdefault(target, schema)
        if schema_versions[target] != schema:
            raise verify.GateError(f"Go/Rust CLI schema versions differ for {target}")
        spec = next(item for item in specs[implementation] if f"{item.os_name}-{item.arch}" == target)
        if proof.get("archive") != spec.archive_name or proof.get("archive_sha256") != sha256(spec.path):
            raise verify.GateError(f"native proof does not bind to package bytes for {key}")
        seen_proofs.add(key)
    if seen_proofs != expected_proofs:
        raise verify.GateError("native proof matrix is incomplete")
    manifest = read_json(dual / "dual-release-manifest.json")
    verify.validate_selection_contract(manifest)
    return {
        "schema_version": 1,
        "version": version,
        "source_revision": source_revision,
        "implementations": implementation_report,
        "targets": sorted({target for _, target in seen_proofs}),
        "signed": False,
        "sbom_scope": "SPDX 2.3 product identity; dependency completeness is not asserted",
        "signing": "blocked; no signatures or certificates were created",
        "publication": "not performed",
        "cutover": "not enabled",
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--merge", action="store_true")
    mode.add_argument("--check", action="store_true")
    parser.add_argument("--packages", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--version", required=True)
    parser.add_argument("--source-revision", required=True)
    args = parser.parse_args()
    try:
        if args.merge:
            if not args.packages or not args.output:
                parser.error("--merge requires --packages and --output")
            result = merge(args.packages.resolve(), args.output.resolve(), args.version.removeprefix("v"), args.source_revision)
        else:
            if not args.candidate:
                parser.error("--check requires --candidate")
            result = check(args.candidate.resolve(), args.version.removeprefix("v"), args.source_revision)
        print(json.dumps(result, indent=2))
        return 0
    except (OSError, ValueError, verify.GateError) as error:
        print(f"BLOCK: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
