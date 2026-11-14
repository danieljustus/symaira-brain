#!/usr/bin/env python3
"""Generate or verify security/rust-hardening-inventory.json (RUST-015).

The inventory lists every resolved production (normal + build) dependency of
the workspace members from `cargo metadata --locked`, every owned `unsafe`
occurrence in workspace Rust sources (including tests, examples and the fuzz
package), and each crate root's `unsafe_code` lint policy. Transitive unsafe in
dependencies is *not* decided here: `scripts/rust_geiger.sh` produces the
reports and the reviewed result is recorded in docs/rust-port/hardening.md.

    rust_hardening_inventory.py --write   regenerate the committed file
    rust_hardening_inventory.py --check   fail when the file is stale (default)

Run through `dev-external` so cargo's build output stays outside the repo.
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
INVENTORY = ROOT / "security" / "rust-hardening-inventory.json"
UNSAFE_TOKEN = re.compile(r"\bunsafe\b")
LINT_POLICY = re.compile(r"#!\[(deny|forbid)\(unsafe_code\)\]")


def metadata() -> dict:
    out = subprocess.run(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path",
         str(ROOT / "Cargo.toml")],
        check=True, capture_output=True, text=True,
    ).stdout
    return json.loads(out)


def production_dependencies(meta: dict) -> list[dict]:
    packages = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    members = set(meta["workspace_members"])
    seen: set[str] = set()
    stack = list(members)
    while stack:
        current = stack.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]["deps"]:
            # kind None = normal dependency, "build" = build script dependency;
            # "dev" edges never reach a shipped binary.
            if any(info["kind"] in (None, "build") for info in dep["dep_kinds"]):
                stack.append(dep["pkg"])
    rows = []
    for pkg_id in seen:
        package = packages[pkg_id]
        source = package["source"]
        rows.append({
            "name": package["name"],
            "version": package["version"],
            "source": "workspace" if source is None else source.split("+", 1)[0],
            "license": package["license"],
            "workspace_member": pkg_id in members,
        })
    return sorted(rows, key=lambda r: (r["name"], r["version"]))


def strip_comments_and_strings(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    text = re.sub(r"//[^\n]*", "", text)
    text = re.sub(r'r(#*)".*?"\1', '""', text, flags=re.S)
    return re.sub(r'"(?:\\.|[^"\\])*"', '""', text)


def owned_unsafe() -> list[dict]:
    hits = []
    for base in (ROOT / "crates", ROOT / "fuzz"):
        for path in sorted(base.rglob("*.rs")):
            if "target" in path.relative_to(ROOT).parts:
                continue
            for number, line in enumerate(
                    strip_comments_and_strings(path.read_text(errors="replace")).splitlines(), 1):
                # `unsafe_code` lint names are policy, not unsafe code.
                if UNSAFE_TOKEN.search(re.sub(r"unsafe_code", "", line)):
                    hits.append({"path": str(path.relative_to(ROOT)), "line": number,
                                 "text": line.strip()})
    return hits


def lint_policy() -> list[dict]:
    rows = []
    for manifest in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        crate = manifest.parent
        root = next((crate / "src" / n for n in ("lib.rs", "main.rs") if (crate / "src" / n).exists()), None)
        match = LINT_POLICY.search(root.read_text()) if root else None
        rows.append({"crate": crate.name,
                     "root": str(root.relative_to(ROOT)) if root else None,
                     "unsafe_code_policy": match.group(1) if match else None})
    return rows


def build() -> dict:
    meta = metadata()
    deps = production_dependencies(meta)
    hits = owned_unsafe()
    policy = lint_policy()
    return {
        "schema": 1,
        "generator": "scripts/rust_hardening_inventory.py",
        "lockfile": "Cargo.lock",
        "owned_unsafe": {"count": len(hits), "locations": hits},
        "unsafe_code_policy": policy,
        "dependencies": {
            "count": len(deps),
            "workspace_members": sum(1 for d in deps if d["workspace_member"]),
            "packages": deps,
        },
        "transitive_unsafe": {
            "status": "requires-cargo-geiger",
            "tool": "scripts/rust_geiger.sh",
            "note": "Per-dependency unsafe use is reported by cargo-geiger and reviewed in "
                    "docs/rust-port/hardening.md; this inventory does not assert it.",
        },
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--write", action="store_true")
    mode.add_argument("--check", action="store_true")
    args = parser.parse_args()
    document = json.dumps(build(), indent=2, sort_keys=True) + "\n"
    summary = json.loads(document)
    if args.write:
        INVENTORY.parent.mkdir(parents=True, exist_ok=True)
        INVENTORY.write_text(document)
        print(f"wrote {INVENTORY.relative_to(ROOT)}")
    elif not INVENTORY.exists() or INVENTORY.read_text() != document:
        print("security/rust-hardening-inventory.json is stale; run with --write", file=sys.stderr)
        return 1
    print(f"inventory: {summary['dependencies']['count']} production packages "
          f"({summary['dependencies']['workspace_members']} workspace), "
          f"{summary['owned_unsafe']['count']} owned unsafe, "
          f"{sum(1 for p in summary['unsafe_code_policy'] if p['unsafe_code_policy'])}/"
          f"{len(summary['unsafe_code_policy'])} crates with an unsafe_code policy")
    bad = [p for p in summary["unsafe_code_policy"] if not p["unsafe_code_policy"]]
    if summary["owned_unsafe"]["count"] or bad:
        for hit in summary["owned_unsafe"]["locations"]:
            print(f"OWNED UNSAFE {hit['path']}:{hit['line']}: {hit['text']}", file=sys.stderr)
        for crate in bad:
            print(f"missing unsafe_code policy: {crate['crate']}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
