#!/usr/bin/env python3
"""Go-free integrity gate for committed Go-oracle fixtures (ORACLE-FREEZE-781).

The Go generators that produced the fixtures are *not* run here. Instead every
fixture is bound to a sha256 manifest with provenance (Go source tree pins, Go
toolchain, recording platform). The gate FAILS - it never skips - when

* a fixture listed in the registry is missing or empty,
* a fixture's bytes differ from the sealed sha256,
* a fixture exists under ``rust/*/tests/fixtures`` but no registry group
  claims it (new oracle output must be registered deliberately),
* a manifest entry lacks provenance, or the manifest and registry disagree.

Sub-commands:
  verify    (default) check the committed manifest; exit 1 on any failure
  seal      recompute sha256/bytes and stamp provenance (no Go needed); used by
            the ``record-*`` Makefile targets after a deliberate re-record

Re-recording the fixtures needs a Go toolchain and lives in ``record.py`` behind
``make record-*`` targets that are NOT part of ``make rust-check``.
"""
from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
REGISTRY = ROOT / "scripts/oracle-freeze/registry.json"
MANIFEST = ROOT / "migration/evidence/oracle-freeze-781/manifest.json"
COVERAGE_GLOB = "rust/*/tests/fixtures/**"
COVERAGE_EXCLUDE = ("rust/symbrain-memory/",)
REQUIRED_PROVENANCE = ("sealed_at_commit", "go_toolchain", "origin", "go_inputs")


class FreezeError(Exception):
    """Raised with a list of human-readable failures."""

    def __init__(self, failures: list[str]):
        super().__init__("\n".join(failures))
        self.failures = failures


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def git(*args: str, root: Path = ROOT) -> str:
    return subprocess.run(["git", *args], cwd=root, check=True, capture_output=True, text=True).stdout.strip()


def load(path: Path) -> dict:
    return json.loads(path.read_text(encoding="utf-8"))


def coverage_files(root: Path) -> set[str]:
    found: set[str] = set()
    for path in root.glob(COVERAGE_GLOB):
        if path.is_file():
            rel = path.relative_to(root).as_posix()
            if not rel.startswith(COVERAGE_EXCLUDE):
                found.add(rel)
    return found


def verify(root: Path = ROOT, registry_path: Path | None = None, manifest_path: Path | None = None,
           groups: set[str] | None = None) -> dict:
    """Return a report dict or raise FreezeError listing every failure.

    With ``groups`` only those groups are checked (an unknown id is a failure);
    the unclaimed-fixture sweep always needs the full set and is skipped then.
    """
    registry = load(registry_path or root / "scripts/oracle-freeze/registry.json")
    manifest = load(manifest_path or root / "migration/evidence/oracle-freeze-781/manifest.json")
    failures: list[str] = []
    reg = {g["id"]: g for g in registry["groups"]}
    man = {g["id"]: g for g in manifest["groups"]}
    if groups is not None:
        for gid in sorted(groups - set(reg)):
            failures.append(f"{gid}: unknown oracle group")
        reg = {k: v for k, v in reg.items() if k in groups}
        man = {k: v for k, v in man.items() if k in groups}
    for gid in sorted(set(reg) - set(man)):
        failures.append(f"{gid}: registered but not sealed in the manifest")
    for gid in sorted(set(man) - set(reg)):
        failures.append(f"{gid}: sealed in the manifest but not registered")
    claimed: dict[str, str] = {}
    fixtures_checked = 0
    for gid in sorted(set(reg) & set(man)):
        group, sealed = reg[gid], man[gid]
        prov = sealed.get("provenance", {})
        for key in REQUIRED_PROVENANCE:
            if not prov.get(key):
                failures.append(f"{gid}: provenance field {key!r} is missing or empty")
        if sealed.get("origin") != group.get("origin"):
            failures.append(f"{gid}: origin changed from {sealed.get('origin')!r} to {group.get('origin')!r}; re-seal deliberately")
        want = {f["path"]: f for f in sealed.get("fixtures", [])}
        if set(want) != set(group["fixtures"]):
            failures.append(f"{gid}: registry fixtures {sorted(set(group['fixtures']) ^ set(want))} differ from the manifest")
        for rel, entry in sorted(want.items()):
            if rel in claimed:
                failures.append(f"{rel}: claimed by both {claimed[rel]} and {gid}")
            claimed[rel] = gid
            path = root / rel
            if not path.is_file():
                failures.append(f"{gid}: fixture missing: {rel}")
                continue
            data_len = path.stat().st_size
            if data_len == 0:
                failures.append(f"{gid}: fixture is empty: {rel}")
                continue
            actual = sha256_file(path)
            fixtures_checked += 1
            if actual != entry["sha256"]:
                failures.append(f"{gid}: sha256 mismatch for {rel}: sealed {entry['sha256'][:16]}.. actual {actual[:16]}..")
            if data_len != entry["bytes"]:
                failures.append(f"{gid}: size mismatch for {rel}: sealed {entry['bytes']} actual {data_len}")
    for rel in sorted(coverage_files(root) - set(claimed) if groups is None else ()):
        failures.append(f"{rel}: fixture is not claimed by any registry group (register + seal it deliberately)")
    if failures:
        raise FreezeError(failures)
    return {"groups": len(man), "fixtures": fixtures_checked,
            "origins": sorted({g["origin"] for g in man.values()})}


def git_tree(rel: str, commit: str = "HEAD") -> str | None:
    try:
        return git("rev-parse", f"{commit}:{rel}")
    except subprocess.CalledProcessError:
        return None


def last_commit(rel: str) -> str:
    try:
        return git("log", "-1", "--format=%H", "--", rel)
    except subprocess.CalledProcessError:
        return ""


def seal(root: Path = ROOT, only: set[str] | None = None, verified: dict | None = None) -> dict:
    """Recompute sha256/bytes and stamp provenance for the registry groups."""
    registry = load(root / "scripts/oracle-freeze/registry.json")
    previous = {}
    if (root / "migration/evidence/oracle-freeze-781/manifest.json").exists():
        previous = {g["id"]: g for g in load(root / "migration/evidence/oracle-freeze-781/manifest.json")["groups"]}
    head = git("rev-parse", "HEAD")
    go_toolchain_default = ""
    for line in (root / "go.mod").read_text().splitlines():
        if line.startswith("go "):
            go_toolchain_default = "go" + line.split()[1]
            break
    groups = []
    for group in registry["groups"]:
        old = previous.get(group["id"])
        if only is not None and group["id"] not in only and old is not None:
            groups.append(old)
            continue
        record = group.get("record", {})
        kind = record.get("kind")
        if kind == "none":
            toolchain = "n/a (hand-authored, no Go involved)"
        elif kind == "script":
            toolchain = record.get("toolchain") or "per producer script (pinned Go revision named by the producer/fixture)"
        else:
            toolchain = record.get("toolchain", "go.mod")
            if toolchain == "go.mod":
                toolchain = go_toolchain_default
        entries = []
        if kind == "copy":
            # A byte-for-byte copy of a Go-repo test input (or of another sealed fixture);
            # refuse to seal a copy that drifted from the source it claims to mirror.
            for dst, src in sorted(record["copies"].items()):
                source_blob = git_tree(src, head)
                if source_blob is not None:
                    dst_blob = subprocess.run(["git", "hash-object", dst], cwd=root, check=True,
                                              capture_output=True, text=True).stdout.strip()
                    if dst_blob != source_blob:
                        raise FreezeError([f"{group['id']}: {dst} is not a byte copy of {src}@HEAD"])
        for rel in group["fixtures"]:
            path = root / rel
            if not path.is_file():
                raise FreezeError([f"{group['id']}: cannot seal missing fixture {rel}"])
            entries.append({"path": rel, "sha256": sha256_file(path), "bytes": path.stat().st_size,
                            "last_modified_commit": last_commit(rel)})
        prov = {
            "sealed_at_commit": head,
            "origin": group["origin"],
            "go_toolchain": toolchain,
            "platform": group.get("platform", "any"),
            "go_inputs": {rel: (git_tree(rel, head) or "absent") for rel in group.get("go_inputs", [])} or {"-": "no Go inputs"},
        }
        if old and old.get("provenance", {}).get("regeneration_verified") and (verified is None or group["id"] not in verified):
            prov["regeneration_verified"] = old["provenance"]["regeneration_verified"]
        if verified and group["id"] in verified:
            prov["regeneration_verified"] = verified[group["id"]]
        groups.append({"id": group["id"], "origin": group["origin"], "description": group["description"],
                       "record": record, "fixtures": entries, "provenance": prov})
    manifest = {"schema_version": 1, "task_id": "ORACLE-FREEZE-781",
                "note": "Go-free fixture manifest. Verify with scripts/oracle-freeze/freeze.py; re-record only via make record-*.",
                "groups": groups}
    out = root / "migration/evidence/oracle-freeze-781/manifest.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(manifest, indent=1) + "\n", encoding="utf-8")
    return manifest


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="cmd")
    verify_parser = sub.add_parser("verify")
    verify_parser.add_argument("--group", action="append", help="verify only these group ids (repeatable)")
    seal_parser = sub.add_parser("seal")
    seal_parser.add_argument("--group", action="append", help="seal only these group ids (default: all)")
    args = parser.parse_args(argv)
    cmd = args.cmd or "verify"
    try:
        if cmd == "verify":
            report = verify(groups=set(args.group) if args.group else None)
            print(f"oracle-freeze: PASS {report['fixtures']} fixtures in {report['groups']} groups (origins: {', '.join(report['origins'])})")
        else:
            manifest = seal(only=set(args.group) if args.group else None)
            print(f"oracle-freeze: sealed {sum(len(g['fixtures']) for g in manifest['groups'])} fixtures in {len(manifest['groups'])} groups")
    except FreezeError as error:
        for failure in error.failures:
            print(f"oracle-freeze: FAIL {failure}", file=sys.stderr)
        print(f"oracle-freeze: {len(error.failures)} failure(s)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
