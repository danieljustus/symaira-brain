# Browse target-stage source provenance

This directory is the Brain receiving copy for PB-2026-09-09's optional Browse
package. It remains a source intake only: the standalone symaira-browse repository,
its `symbrowse` distribution and supported compatibility routes are unchanged.

- **Source repository:** `github.com/danieljustus/symaira-browse`
- **Source commit:** `7884cdde9d842c44ae8624b148122fdfa4caa019`
- **Source subject:** `fix(bench): restore paired static transport measurements (#436)`
- **Source path:** repository root
- **Receiving path:** `symaira-brain/browse/`
- **Support source:** none; Browse is self-contained.
- **Imported tracked files:** **585**. The complete eligible inventory is:
  `cmd/` 76, `crates/` 122 (the full 11-member Rust workspace), `internal/`
  328, `port/` 10, `formflow/` 14, `docs/` 27 (Markdown only, including all
  `docs/rust-port/*.md`), `scripts/rust-port/test_benchmark_harness.py` 1,
  and 7 top-level files (`.gitignore`, `README.md`, `go.mod`, `go.sum`,
  `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`).

The resync imports the four files changed or added by Browse PR436:
`crates/symbrowse-cli/src/main.rs`, `docs/rust-port/rust016-release-gates.md`,
`port/bench/run.py`, and `scripts/rust-port/test_benchmark_harness.py`. The last
file is an explicit exception to the standing scripts exclusion because it is
the PR's added benchmark regression test and is required for regression inclusion.
All other eligible files are refreshed from the exact source commit as well.

## Intentional receiver adaptations

These three eligible paths remain receiver-specific and are not byte-identical:

1. `browse/.gitignore` is adapted for the receiving repository's local layout and
   build-storage conventions.
2. `browse/go.mod` pins `github.com/danieljustus/symaira-corekit` at `v0.17.0`,
   while Brain's root module pins `v0.16.2`. Browse declares its own module, so
   these are independent module-graph facts, not a root build conflict.
3. `browse/internal/policy/policy_test.go` was formatted with `gofmt -w -s` after
   the original intake because the source file was not gofmt-clean.

The imported count includes these three adapted paths. Every other imported path's
content and mode must match the source tree exactly.

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
