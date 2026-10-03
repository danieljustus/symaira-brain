# Native memory evidence acceptance (#758)

`PATH=/path/to/go1.26.7/bin:$PATH scripts/memory-evidence-oracle/run.sh --report /tmp/evidence-report.json`

The supplemental runner archives the immutable Brain Go revision
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, verifies the exact CoreKit
`evidencekit` v0.17.0 source hash, and executes its production alignment,
validation and JSONL encoder. It compares all bytes against an additive fixture;
it does not rewrite existing Go source or frozen fixtures. All application
HOME/XDG paths belong to a disposable directory. Toolchain caches remain reusable.

The fixture records 32 source/evidence pairs, all four alignment entry points,
48 strict/opt-in validation cases, and three JSONL records with a byte SHA-256.
Cases include multibyte UTF-8 offsets, Unicode whitespace, zero-width non-spaces,
fuzzy edit-distance thresholds, and equal-score windows. Existing Go database
tests run against the production implementation with a required count of four.

Run Rust contracts with `cargo test -p symbrain-memory --all-targets --all-features
--locked` and strict Clippy for the same targets/features. Native Linux, macOS,
and Windows execute this gate in `memory-evidence-native.yml`; those receipts
are required before accepting this increment.

Run all three actual negative processes with
`python3 scripts/memory-evidence-oracle/controls.py --report /tmp/controls.json`.
CI requires them on every native target and retains their failure logs.

For a negative control, use `--fixture` with an owned missing file or a temporary
copy whose status, count or encoded JSONL byte has changed. Each must fail with
a nonzero process exit. `--fixture` is not used by acceptance CI.

This increment supplies alignment, strict validation, JSONL **encoding** and
memory-evidence persistence. A Go-compatible JSONL decoder, full memory
configuration/CLI flag and error parity, and native CLI cutover remain open.
The existing Go fallback routes are retained; #758 and release-dependent #649
must not be closed on these results alone.
