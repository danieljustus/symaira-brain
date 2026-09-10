# Browse target-stage source provenance

Resynced on 2026-09-10 from symaira-browse `main` at the pinned commit above.

This directory is the Brain receiving copy for PB-2026-09-09's eventual
optional Browse package. It preserves the symaira-browse source tree, with
exactly one intentional, reviewed deviation from the source commit:

1. `browse/go.mod` pins `github.com/danieljustus/symaira-corekit` at
   `v0.17.0`, while this repository's (symaira-brain's) root `go.mod` pins
   `v0.16.2`. `browse/go.mod` declares its own Go module
   (`module github.com/danieljustus/symaira-browse`), so this is a fact
   about two independent module graphs, not a build conflict: `go build
   ./...`, `go vet ./...` and `go test ./...` run from this repository's
   root do not descend into a directory with its own `go.mod` (Go
   module-boundary semantics — confirmed, no config change made to force
   or prevent this). Unlike the Scope intake, which could report an
   identical pin with no divergence, this is simply documented as-is.

All other imported tracked files are expected to match the source commit
byte-for-byte.

- **Source repository:** `github.com/danieljustus/symaira-browse`
- **Source commit:** `1a383d8651e1461712a4da43cdf0683884e46a3a` (`ci: avoid
  unsupported Windows pinned Go Unix-socket parity (#435)`) — this was the
  current `main` HEAD of symaira-browse at resync time; verified with `git rev-parse origin/main` in the source
  repository and by a clean-checkout `diff -rq` verification.
- **Source path:** repository root
- **Receiving path:** `symaira-brain/browse/`
- **Support source:** none. Unlike Operate and Scope, which both depend on
  the shared `history/` package (received separately at
  `symaira-brain/history/` by the Scope intake, #551), Browse is
  self-contained — it has no equivalent shared-library dependency carried
  in this repository.
- **Imported tracked files:** 584 under `browse/` — `cmd/` (76), `crates/`
  (122, the full 11-member Rust workspace: `symbrowse-cli`,
  `symbrowse-compat`, `symbrowse-core`, `symbrowse-daemon`,
  `symbrowse-engine`, `symbrowse-engine-chrome`, `symbrowse-engine-safari`,
  `symbrowse-engine-firefox`, `symbrowse-fetch`, `symbrowse-mcp`,
  `symbrowse-protocol`), `internal/` (328, all packages unchanged, including
  the three go:embed targets `internal/injection/patterns.txt`,
  `internal/engine/devices.json`, and
  `internal/engine/axe/assets/axe.min.js` — all self-contained inside
  `internal/`), `port/` (11), `formflow/` (14), 27 of the 36 files under
  `docs/` (every `docs/*.md` file, recursively, including all of
  `docs/rust-port/*.md`), plus `README.md`, `go.mod`, `go.sum`,
  `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and an adapted
  `.gitignore` (1 file each, 7 total).
- **Excluded (not imported), with reasons:**
  - `CHANGELOG.md`, `LICENSE`, `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`,
    `SECURITY.md`, `.golangci.yml`, `.goreleaser.yml`, `.editorconfig`,
    `deny.toml` — repo-root boilerplate not needed for an in-repo receiving
    package.
  - `assets/` — brand assets only, confirmed unused by any `go:embed`
    directive.
  - `testdata/` — test-only fixtures.
  - `security/` — a generated audit artifact
    (`rust-hardening-inventory.json`), not source.
  - `scripts/` — release/audit helper scripts, not needed for a build-only
    package.
  - `e2e/` — one smoke test, not run here.
  - `fuzz/` — its own nested `cargo-fuzz` `Cargo.toml`, a third Cargo
    workspace layer. Explicitly excluded, not needed for a build-only
    receiving copy — this is a deliberate omission, not an oversight.
  - `docs/assets/social-preview.svg` and the eight non-Markdown working
    files under `docs/rust-port/` (`baseline.json`, `contract-matrix.json`,
    `rust009-tls-results.json`, `rust011-cdp-probe.json`,
    `rust011-value-signal.json`, `validate.py`, `value-signal-version.json`,
    `work-items.json`) — the include set was "the per-topic design docs
    directory, `.md` files"; these are a brand asset and generated
    tracking/validation artifacts for the Rust-port effort, not design docs.
  - `AGENTS.md`, `CLAUDE.md`, `.gitattributes`, `.github/` — not part of the
    defined include set (source repo agent-instruction and CI-workflow
    files; this repository's own `AGENTS.md`/CI already govern the
    receiving copy).
  - `.git/`, `.worktrees/`, `dist/`, the `target` symlink to the external
    build cache, and any other untracked/gitignored files — never
    considered; the copy was taken with `git archive` from the pinned
    commit, which only emits tracked, committed content. The local
    `symbrowse` binary is gitignored and was confirmed absent from `git
    ls-files` in the source repository before the copy; it was not carried.
- **Manifest pins:** see deviation 1 above (`symaira-corekit` `v0.17.0` in
  `browse/go.mod` vs. `v0.16.2` in this repository's root `go.mod` — an
  independent-module fact, not a conflict).
- **Upstream state not pulled in:** a separate, unmerged branch in
  symaira-browse, `fix/pb-windows-test-paths-20260910`, was open at intake
  time with a failing required CI check
  (`crates/symbrowse-mcp/tests/raw_frames.rs`, a Windows path-separator
  issue). It was deliberately ignored — this intake pins to `main` only, per
  the task brief. A re-sync of this receiving copy will be needed once that
  branch merges upstream.

The original `symaira-browse` source, `symbrowse` CLI/MCP binary, its Rust
workspace, installed binaries, configuration, and data remain supported and
untouched. This is source intake and an independently runnable target
package, not a consumer cutover or release.

The package stays optional: nothing in Brain's startup path
(`internal/config.ModulesConfig`, `internal/managed/manifest.json`, or any
other consumer path) imports or runs it. Brain's existing `[servers.browse]`
foreign-server route — spawning the standalone `symbrowse` binary, already
supported end-to-end today by `internal/config.ModulesConfig.Browse` and
`internal/managed` (verified previously) — remains the actually-wired,
authoritative route. A future re-sync will be needed once
`fix/pb-windows-test-paths-20260910` merges upstream.
