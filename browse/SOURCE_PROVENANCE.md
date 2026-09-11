# Browse target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09's optional Browse
package. It remains a source intake only: the standalone symaira-browse repository,
its `symbrowse` distribution and supported compatibility routes are unchanged.

- **Source repository:** `github.com/danieljustus/symaira-browse`
- **Source commit:** `62fca84a83190434fa14b514a5654fc65dc2e396`
- **Source subject:** `fix(browser): report Safari capabilities and unsupported interactions`
- **Resync date:** 2026-09-11
- **Source path:** repository root
- **Receiving path:** `symaira-brain/browse/`
- **Support source:** none; Browse is self-contained.
- **Imported tracked files:** **585**. The complete eligible inventory is:
  `cmd/` 76, `crates/` 122 (the full 11-member Rust workspace), `internal/`
  328, `port/` 10, `formflow/` 14, `docs/` 27 (Markdown only, including all
  `docs/rust-port/*.md`), `scripts/rust-port/test_benchmark_harness.py` 1,
  and 7 top-level files (`.gitignore`, `README.md`, `go.mod`, `go.sum`,
  `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`).

The resync refreshes the five stale Safari/BiDi receiver files from the exact
pinned Git tree:
`crates/symbrowse-daemon/src/runtime.rs`,
`crates/symbrowse-daemon/src/safari_runtime.rs`,
`crates/symbrowse-daemon/tests/safari_native.rs`,
`crates/symbrowse-engine-safari/src/bidi.rs`, and
`crates/symbrowse-engine-safari/tests/bidi.rs`.

## Intentional receiver adaptations

The receiving copy preserves these receiver conventions:

1. `browse/.gitignore` is adapted for the receiving repository's local layout and
   build-storage conventions. It is the only content difference in the current
   full comparison.
2. `browse/go.mod` retains the receiver's independently reviewed CoreKit pin
   (`v0.17.0`); it matches the pinned source tree and remains an independent
   nested module graph relative to Brain's root `go.mod` (`v0.16.2`).
3. `browse/internal/policy/policy_test.go` retains the receiver's gofmt-formatted
   state; it matches the pinned source tree at this resync.

## Full provenance comparison

The 585 eligible target paths were compared programmatically against the pinned
source tree by complete relative path, file mode, and blob content. Result:
**585/585 paths present, 0 missing, 0 extra; 585/585 modes equal; 584/585
content-identical; 1 intentional content adaptation (`.gitignore`); 0
unexplained differences.** The comparison was performed after the five-file
resync and includes the preserved receiver adaptations above.

## Exclusions

The source commit contains 748 tracked files; 163 are intentionally excluded:

- Root boilerplate and policy/instruction files: `CHANGELOG.md`, `LICENSE`,
  `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, `SECURITY.md`, `.golangci.yml`,
  `.goreleaser.yml`, `.editorconfig`, `deny.toml`, `Makefile`, `AGENTS.md`,
  `CLAUDE.md`, and `.gitattributes`.
- `.github/`, `assets/`, `testdata/`, `security/`, `e2e/`, and `fuzz/` trees.
- Release/audit helper scripts under `scripts/`, except the explicitly imported
  `scripts/rust-port/test_benchmark_harness.py` regression above.
- `docs/assets/social-preview.svg` and the eight non-Markdown files under
  `docs/rust-port/` (`baseline.json`, `contract-matrix.json`,
  `rust009-tls-results.json`, `rust011-cdp-probe.json`,
  `rust011-value-signal.json`, `validate.py`, `value-signal-version.json`,
  `work-items.json`).
- `port/results/rust016-benchmark-v2.json`, a generated benchmark result that is
  not source and remains excluded despite being tracked upstream.
- `.git/`, `.worktrees/`, `dist/`, the external `target` build-cache symlink and
  other untracked or ignored state are never considered.

No protected instruction, distribution, release, identity or source-repository
file is copied. No Brain startup path, module manifest, consumer route or
standalone distribution surface is changed; Browse remains optional and
non-consumer-wired.
