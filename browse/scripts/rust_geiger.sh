#!/usr/bin/env bash
# cargo-geiger over every production workspace crate (RUST-015).
#
# The root manifest is virtual, so geiger runs once per member manifest and
# writes one JSON report per package to <target>/hardening/geiger/. The
# reports are CI artifacts, never hand-written. The script fails when an owned
# (workspace) crate contains any unsafe code; unsafe in third-party
# dependencies is summarized for review, not failed on, because the stop rule
# only forbids *owned* unsafe. Run through `dev-external` so build output stays
# outside the repository.
#   GEIGER_VERSION  expected cargo-geiger version (default: 0.13.0)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

expected="${GEIGER_VERSION:-0.13.0}"
actual="$(cargo geiger --version | awk '{print $NF}')"
if [ "$actual" != "$expected" ]; then
  echo "cargo-geiger $expected required (found $actual); install with: cargo install cargo-geiger --version $expected --locked" >&2
  exit 2
fi

out="${CARGO_TARGET_DIR:-$root/target}/hardening/geiger"
mkdir -p "$out"
rm -f "$out"/*.json

for manifest in "$root"/crates/*/Cargo.toml; do
  crate="$(basename "$(dirname "$manifest")")"
  cargo geiger --manifest-path "$manifest" --all-features --locked \
    --output-format Json >"$out/$crate.json" 2>"$out/$crate.stderr"
  echo "geiger: $crate -> $out/$crate.json"
done

python3 - "$out" "$root" <<'PYEOF'
import json
import sys
from pathlib import Path

out, root = Path(sys.argv[1]), Path(sys.argv[2])
owned_unsafe, rows = [], []
for report in sorted(out.glob("*.json")):
    data = json.loads(report.read_text())
    for entry in data["packages"]:
        ident = entry["package"]["id"]
        used, unused = entry["unsafety"]["used"], entry["unsafety"]["unused"]
        count = sum(
            bucket[kind]["unsafe_"]
            for bucket in (used, unused)
            for kind in ("functions", "exprs", "item_impls", "item_traits", "methods")
        )
        source = ident["source"]
        is_owned = isinstance(source, dict) and "Path" in source
        if is_owned and count:
            owned_unsafe.append((report.stem, ident["name"], count))
        rows.append((ident["name"], ident["version"], count, entry["unsafety"]["forbids_unsafe"]))
unsafe_deps = {(n, v) for n, v, c, _ in rows if c}
print(f"geiger: {len({(n, v) for n, v, _, _ in rows})} distinct packages scanned, "
      f"{len(unsafe_deps)} contain unsafe (reviewed in docs/rust-port/hardening.md)")
if owned_unsafe:
    for report, name, count in owned_unsafe:
        print(f"OWNED UNSAFE: {name} ({count} items) in {report}.json", file=sys.stderr)
    sys.exit(1)
PYEOF
