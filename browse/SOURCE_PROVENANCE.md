# Browse target-stage source provenance

This directory is the Brain-owned Browse source for PB-2026-09-09's optional Browse package. The source/consumer cutover completed on 2026-09-13: `symaira-browse` is archived and Brain builds/manages `symbrowse` from this receiving tree. No signed Brain-side release or package-manager replacement is implied.

- **Source repository:** `github.com/danieljustus/symaira-browse`
- **Source commit:** `e86c1db46ad758d89372640473a5311525e3edf1`
- **Source subject:** `fix(safari): wait for the Go oracle navigation target (#440)`
- **Archive reference:** `c9ab83cc5cc667b56f8b18982611996ebd09c6f6` (historical only; not the receiving source)
- **Resync date:** 2026-09-13
- **Source path:** repository root
- **Receiving path:** `symaira-brain/browse/`
- **Support source:** none; Browse is self-contained.
- **Eligible source inventory:** **586** paths. The complete eligible inventory is:
  `cmd/` 76, `crates/` 122 (the full 11-member Rust workspace), `internal/`
  328, `port/` 10, `formflow/` 14, `docs/` 27 (Markdown only, including all
  `docs/rust-port/*.md`), `scripts/rust-port/test_benchmark_harness.py` 1,
  and 8 top-level files (`.gitignore`, `README.md`, `CONTRIBUTING.md`, `go.mod`,
  `go.sum`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`).

The Batch A overlay additionally contains eight historical register/evidence
files under `browse/docs/rust-port/`. They are not part of the 586-path source
inventory or a current acceptance total. Their source blobs are recorded below;
the overlay was initially imported byte-identically, with only the validator
and work-items register receiving the narrow receiver repairs below.

The resync applies exactly the four eligible files changed between the old pin
`62fca84a83190434fa14b514a5654fc65dc2e396` and the new pin, read from Git
objects only (the source working tree is dirty):
`crates/symbrowse-engine-safari/src/attach.rs`,
`crates/symbrowse-engine-safari/tests/attach.rs`,
`crates/symbrowse-engine-safari/tests/contract_fixture.rs`, and
`internal/engine/safari/rust_contract_fixture_test.go`. The two changed
`testdata/` files are intentionally excluded by the intake policy and were not
imported.

## Intentional receiver adaptations

The receiving copy preserves these receiver conventions:

1. `browse/.gitignore` is adapted for the receiving repository's local layout and
   build-storage conventions.
2. `browse/cmd/symbrowse/readme_commands_test.go` is narrowly adapted to check
   only the documentation files shipped in the nested module (`README.md` and
   `CONTRIBUTING.md`); the upstream test's checks for excluded `AGENTS.md` and
   `.golangci.yml` are not retained. This is a fixture-path test adaptation, not
   a product behavior change.

`browse/CONTRIBUTING.md` is copied byte-for-byte as an explicit tracked-document
exception because `browse/README.md` links to it and the nested README/version
test reads it relative to the Browse module root. Source blob:
`c84daf7fe07b08a82f72b88e0a51ebc0f79a4720`. No `AGENTS.md`, `SECURITY.md`,
`LICENSE`, or global policy file is copied or renamed.

## Full provenance comparison

The 586 eligible target paths were compared programmatically against the pinned
source tree by complete relative path, file mode, and blob content at intake.
That result was **586/586 paths present, 0 missing, 0 extra; 586/586 modes
equal; 584/586 content-identical; 2 intentional content adaptations
(`.gitignore` and `cmd/symbrowse/readme_commands_test.go`); 0 unexplained
differences.** The current bounded handoff adds two further documented
receiver adaptations inside that inventory (`docs/rust-port/rust011-cdp-feasibility.md`
and `port/harness/run.py`); the validator/work-items repairs and test overlays
are outside the 586-path source inventory.

The eight-file Batch A overlay was checked separately against Git objects from
the same source commit:

| Receiving path | Source path | Source blob |
|---|---|---|
| `browse/docs/rust-port/contract-matrix.json` | `docs/rust-port/contract-matrix.json` | `a33e4b78c7f6eca2e3a213ab302f34530db8e901` |
| `browse/docs/rust-port/work-items.json` | `docs/rust-port/work-items.json` | `3080f500c61dde8712f92f8b6857d21838446358` |
| `browse/docs/rust-port/validate.py` | `docs/rust-port/validate.py` | `01e3db25d3116f611693f3a6b1decf36248947ed` |
| `browse/docs/rust-port/baseline.json` | `docs/rust-port/baseline.json` | `0e101f917696733a7e7ae2edfa7de2668a2fdcc3` |
| `browse/docs/rust-port/value-signal-version.json` | `docs/rust-port/value-signal-version.json` | `a827267800ef07796a00d4c7ad04f320aa9e4ffa` |
| `browse/docs/rust-port/rust009-tls-results.json` | `docs/rust-port/rust009-tls-results.json` | `ebd2bec169b4c1aebc546672ae2fbcb8693a3e1c` |
| `browse/docs/rust-port/rust011-cdp-probe.json` | `docs/rust-port/rust011-cdp-probe.json` | `908a0c08186f235a555de4855e113e35e1a3e060` |
| `browse/docs/rust-port/rust011-value-signal.json` | `docs/rust-port/rust011-value-signal.json` | `4c11f27836eec298cba118f5b15a9c07d72759b3` |

The baseline, TLS result, CDP probe and value-signal JSON files are historical
evidence. Recorded `pass` fields describe those old runs only; they are
unverified on the current Brain candidate and are not current acceptance.
The matrix and work-items files are historical register inputs; their current
IDs, links and dependency graph are checked by `validate.py` without treating
their status fields as release approval.

The Batch B closure overlay adds only the 13 generator/fixture paths that
`browse/port/harness/run.py` directly requires: five `scripts/rust-port/`
generators, the transport-selection fixture, the engine fixture plus manifest,
the Chrome and Safari fixture/manifest pairs, and the workflow fixture. Each
was copied from the same pinned Git commit; these are test-harness inputs, not
a broad Browser product port. The harness also has one receiver-only branch
for the existing `symbrowse-engine-chrome` `spike` test.

## FETCH-002 candidate check — 2026-09-16

The current Brain candidate was checked for the legacy six-profile wire
compatibility dependency before any new fixture or suite was added. The
source-bound fingerprint suite is **missing**: `browse/port/harness/run.py`
contains no `fetch-fingerprints` suite or fingerprint branch, and the existing
`compat-sidecar` suite covers only the versioned NDJSON sidecar lifecycle. The
existing Go test `internal/fetch/fetch/fetch_test.go`
(`TestAzureTLSHeaderOrder`) compares AzureTLS Chrome against honest HTTP/1.1
header order only; it does not capture TLS ClientHello, HTTP/2 settings, all
six profiles, or a pinned source oracle.

A productive compatibility path does exist, but it is not fingerprint-suite
evidence: `cmd/symbrowse/compat_sidecar.go` constructs the Go AzureTLS client
from the requested profile, and `crates/symbrowse-daemon/src/runtime.rs` calls
the Rust `CompatClient` through the versioned sidecar boundary. The tracked
`port/harness/cases/compat-sidecar.json` has eight protocol/lifecycle cases,
not FETCH-002 wire cases. The historical `rust009-tls-results.json` remains
invalidated and unverified on this candidate; its recorded comparisons are not
current acceptance.

Therefore no `fetch-fingerprints` suite, fingerprint fixture, or historical
PASS was invented. RUST-009 is recorded as `blocked` in `work-items.json`.
The next existing command is `python3 port/harness/run.py --suite
compat-sidecar` for sidecar protocol/lifecycle checks only. FETCH-002 next
needs a source-bound capture fixture and executable six-profile comparison
from the pinned Go oracle before any native wire check or compat replacement.

The pinned source repository was independently re-read at commit
`e86c1db46ad758d89372640473a5311525e3edf1` (748 tracked paths). That exact
source tree confirms the same gap: `port/harness/run.py` is blob
`fcc6564d458fba8d19570f9dd05843ce07a43618` and has no fingerprint suite;
`internal/fetch/fetch/fetch_test.go` is blob
`cf97aa5fb69a573c5f14e28bed9acb7df72bb393` and has only the Chrome-vs-honest
HTTP/1.1 `TestAzureTLSHeaderOrder` capture. The retained
`docs/rust-port/rust009-tls-results.json` fixture is blob
`ebd2bec169b4c1aebc546672ae2fbcb8693a3e1c` and has six profile rows, but its
verdict is `INVALIDATED`; it is recorded comparison output without a raw
ClientHello capture, generator, or source/generator digest contract. The
source's `scripts/rust-port/internal/diff/capture.go` is a bounded process
stream buffer and `cmd/diffharness` compares generic black-box outputs; neither
is a TLS/HTTP/2 wire-capture generator. These existing artifacts therefore
cannot be promoted into a current source-bound FETCH-002 suite without a new
pinned Go capture intake.

The source commit contains 748 tracked files; 162 are intentionally excluded:

- Root boilerplate and policy/instruction files: `CHANGELOG.md`, `LICENSE`,
  `CODE_OF_CONDUCT.md`, `SECURITY.md`, `.golangci.yml`,
  `.goreleaser.yml`, `.editorconfig`, `deny.toml`, `Makefile`, `AGENTS.md`,
  `CLAUDE.md`, and `.gitattributes`.
- `.github/`, `assets/`, `testdata/`, `security/`, `e2e/`, and `fuzz/` trees,
  except the eight explicitly imported `testdata/port/` closure fixtures listed
  in the Batch B handoff manifest.
- Release/audit helper scripts under `scripts/`, except the explicitly imported
  `scripts/rust-port/test_benchmark_harness.py` regression and the five
  `scripts/rust-port/` closure generators listed in that manifest.
- `docs/assets/social-preview.svg` and the eight non-Markdown files under
  `docs/rust-port/` were excluded from the 2026-09-13 source resync. Those
  eight files are now present only as the separately bound historical Batch A
  overlay listed above; they do not expand the 586-path source inventory or
  establish current acceptance.
- `port/results/rust016-benchmark-v2.json`, a generated benchmark result that is
  not source and remains excluded despite being tracked upstream.
- `.git/`, `.worktrees/`, `dist/`, the external `target` build-cache symlink and
  other untracked or ignored state are never considered.

No protected instruction, distribution, release, identity or source-repository
file is copied. No Brain startup path, module manifest or standalone
distribution surface is changed by this provenance record; Browse remains
optional and any consumer wiring remains an explicit Brain module/profile
choice.
