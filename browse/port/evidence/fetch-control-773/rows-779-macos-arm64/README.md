# #779 fresh frozen-Go/native fetch comparison (macOS arm64, 2026-11-16)

Re-run of the #773 process corpus against the current Rust tree, to back
FETCH-003/004/005/009/010 with a live comparison instead of only the 2026-11
receipts next to this directory.

- Go oracle: source commit `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`
  (`git archive`), plus the supplemental probe `fetch_control_773.go.txt` taken
  from `dde8cced1^` (sha256 `9e0729730cedd8402b0aeff0604ad0a56bde67f0178d5391df076c49c071f3d4`), built with `go build -trimpath`,
  CGO off, **go1.27.4** (the pinned go1.26.7 toolchain is not installed; this
  deviation is not hidden: it is a newer Go net/http and every case still matched).
- Rust: `cargo build -p symbrowse-fetch --example control_probe` at
  `5c1fcb490` (macOS arm64, debug build). The report's `candidate_dirty: true`
  is the harness copy of the probe source that the script hashes; it was removed
  again and is not part of the tree.
- `control-report.json`: 251/251 matched (74 private HTTP exchanges, 177
  route-only selections). `control-negative.json`: five native-output mutants
  rejected. `raw-proxy*.json`: 32 exact pairs / three mutants rejected.
  `proxy-auth*.json`: 66 ordered-header pairs and 8 accepted-auth pairs / three
  mutants rejected.
- Not covered here: Linux (receipts of 2026-11-10 in the parent directory),
  SOCKS (Rust behaviour test only; the Go honest client has no SOCKS corpus),
  named cookie jars and br/zstd (native target features; Go does not implement
  them).
