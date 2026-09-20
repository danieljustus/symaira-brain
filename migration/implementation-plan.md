# Symaira Brain Go-to-Rust Migration Implementation Plan

## Resume checkpoint — 2026-09-20, wave 3: DB-001 divergence measured

`DB-001`/`DB-002` moved from "no consumer" to "measured divergence": the native
store is now held to the frozen Go facts by an executable differential test,
and that test is red. The row stays `fixture-ready` until it is green.

- **Acceptance test written** (`rust/symbrain-memory/src/db_oracle_tests.rs`,
  inside the crate because `busy_timeout` and `foreign_keys` are
  connection-scoped and can only be read from the store's own connection).
  Passing today: the lock pragmas (`busy_timeout` 5000, `journal_mode` wal,
  `foreign_keys` 1, `secure_delete` 1), the constraint cases (NULL
  `created_at`/`updated_at` rejected, missing table rejected) and the ordering
  cases (ties broken by id DESC, distinct timestamps by `created_at` DESC).
- **Closed** (commit `76a14f3a`, on the integration branch). The port landed
  with the acceptance test, because the test alone was red and the DDL alone
  was unmeasured. `DB-001`/`DB-002` are now `green`:
  1. **16 tables** added to the native schema: `audit_log`,
     `consolidation_runs`, `context_profile_links`, `context_profiles`,
     `entities_aliases`, `import_state`, `jwt_revocations`, `memories_fts`
     (+ its four shadow tables), `memory_associations`, `memory_evidence`,
     `query_log_results`, `sync_state`.
  2. **`memories` column facts** aligned: the shipped order and the shipped
     defaults. The native schema had added `DEFAULT ''` to `scope`, `metadata`
     and `embedding`, which the shipped schema declares NOT NULL without a
     default.
  3. **16 `memories` indexes** added (kind, tier, review_status, expires_at,
     content_hash, embedding_source, consolidation, consolidated_into,
     created_by, importance, lsh, scope_lsh, superseded_by, updated_at,
     valid_from, valid_to) plus the unique autoindex the shipped table carries.
- **Two defects in the worker's port were caught by existing tests, not by the
  worker's own report** — its summary claimed "all three checks pass" and did
  not mention them, and clippy/fmt were red:
  - the 16 new indexes sat inside `SCHEMA`, ahead of the `COLUMN_PARITY`
    repair, so `Store::open` failed on any database written by an older
    version (`no such column: tier`) — the indexes now live in `INDEXES` and
    run after the repair;
  - `COLUMN_PARITY` covered 5 columns while the shipped index set references
    26, so an upgraded database would have kept missing columns a fresh one
    has — it now covers every shipped `memories` column with its shipped
    definition;
  - a third gap **no test could see**: `memories_fts` was created without the
    `memories_ai`/`_ad`/`_au` triggers, so the table existed and never received
    a row. The oracle now records triggers and views (stored SQL; neither has a
    PRAGMA), the differential test holds them to it, and a new contract test
    proves a natively written row reaches the FTS index.
  - the contract test's raw insert omitted `embedding`, which only worked
    while the native schema wrongly gave that column a default.
- **Verification on the integration branch:** `cargo test -p symbrain-memory`
  21 lib + 7 contract, `clippy --all-targets -D warnings` clean,
  `cargo fmt --check` clean, `make rust-check` exit 0 (all oracle checks: xdg
  256, policy 50+30, guard 51, guard-doctor 7, catalog 3, cli 81, db-memory
  29 tables / 4 negative / 2 ordering), `make parity-smoke` 457/457.
- **The oracle was repaired first**, because three of its own cases were
  hollow: the ordering seeds omitted `metadata` (NOT NULL without a default),
  so the inserts failed silently and both expectations were captured as
  `null`; the NULL-timestamp cases were rejected for `metadata` rather than for
  the timestamp they name; and the legacy-migration case was dropped because
  the oracle opened the legacy file with the driver name `sqlite3` while the
  production package registers modernc's `sqlite`. Insert and open errors are
  now fatal. Note that `make db-memory-oracle-check` exercises the **committed**
  oracle (it exports `HEAD`), so an uncommitted oracle change is not covered by
  it — regenerate and commit the fixture explicitly.
- **Reachability, verified with a freshly built native binary and an isolated
  `HOME`:** `symbrain memory …` is gated back to the shipped implementation and
  resolves `<data>/memory/default.db` (it does **not** create a store through
  the native code path), but `symbrain mcp` **is** live: the gateway opens
  `symbrain_memory::Store` on the same `<data>/memory/default.db`
  (`rust/symbrain-gateway/src/lib.rs`). An earlier probe of mine that suggested
  the CLI path was native used a stale binary from 2026-09-17 and was wrong; the
  corrected measurement is in #626, which the port closes.
- **Branch state:** everything above is integrated on
  `migration/rust-continue-20260920` (local, not pushed). The wave-3 worktree
  `migration/w3-db-schema` has served its purpose and is removed. Nothing about
  this slice is outstanding.

## Resume checkpoint — 2026-09-20, wave 2 salvaged

Both wave-2 workers reported `completed` but neither had committed, and one had
derailed mid-task. Their branches were empty; the artifacts were found in the
worktrees (one of them in the **shared coordinator checkout**) and were
verified by hand before anything was kept.

- **`DB-001`/`DB-002` → `fixture-ready`.** `scripts/db-memory-oracle` imports
  `internal/memory/config` and `internal/memory/db`, creates a real store and
  reads the schema, pragmas and constraint behaviour back out of SQLite:
  29 tables, the full `memories` column set, 5 lock pragmas, and the NULL
  timestamp, ordering and tie-break cases. `make db-memory-oracle-check` →
  "0 drift on 29 tables, 3 negative cases, 2 ordering cases". No Rust test
  consumes the fixture yet, so the rows are not green.
- **`CLI-006` → `fixture-ready`.** `scripts/cli-oracle` freezes 81 cases of
  command-tree and flag behaviour (measured stdout, stderr, exit codes,
  including the negative flag cases), with the readable inventory in
  `migration/cli-tree-inventory.md`. `make cli-oracle-check` → "0 drift on
  81 cases".
- **The CLI oracle as the worker left it was unsafe and was rebuilt.** It ran
  the real binary with the ambient environment, so its own run executed
  `setup` (downloading and installing managed binaries into the operator's
  `~/.symaira/bin`), `doctor --fix`, and let `sync` write managed blocks into
  this repository's `AGENTS.md`. It is now isolated (throwaway `HOME`/`XDG`,
  scratch working directory), the two side-effecting cases are dropped, and
  the per-run root, the checkout path, the toolchain line and the host platform
  are placeholders. The fixture also carried 50 self-contradicting
  descriptions (a dead `expectedExit` parameter asserting exit 64/100 while the
  measured code was 2); the dead parameter is gone and the descriptions no
  longer claim an exit code.
- **Side effect on the operator's machine, reported:** `symdesk` v0.12.2 and
  `symvault` v0.22.1 were installed into `~/.symaira/bin` at 14:27 by that
  `setup` run (`symbrowse`/`symoperate`/`symscope` untouched from Sep 13).
- **New defect filed: #625.** `symbrain setup` cannot install the pinned
  `symcockpit` core: `managed: extract symcockpit: unsafe tar entry "./":
  invalid archive path`. Reproduced with an isolated `HOME` so the operator's
  binaries were not touched a second time.
- **Process lesson for the next wave:** a worker's artifact must be inspected
  and its side effects understood *before* it is run; an oracle that mutates
  the machine or the checkout is not evidence. Ask workers to commit as soon as
  the first slice passes, because a cut-off worker leaves nothing on its branch.

## Resume checkpoint — 2026-09-20, wave 1 integrated

Three workers were dispatched in isolated worktrees from `8ca8d2c5`. Two of
them were cut off before committing and their summaries arrived truncated, so
their branches were inspected and salvaged by hand instead of being trusted.

- **`CFG-001` is green.** `scripts/xdg-oracle` is a source-bound Go oracle
  (imports `internal/xdg`, injects HOME/XDG, 256 cases) and
  `rust/symbrain-core/tests/xdg_paths_tests.rs` compares all eight resolution
  functions against the frozen fixture. Verified on the integrated tree by the
  author: `run-go-oracle.sh HEAD run ./scripts/xdg-oracle -check` →
  "0 drift on 256 cases", and `cargo test -p symbrain-core` →
  `xdg_paths_match_go_oracle ... ok`. No divergence was found; no Rust change
  was needed. `make xdg-oracle-check` is wired into `rust-check`.
- **`#624` (atime flake) is addressed in the harness.** The three
  atime-derived cases now capture each runtime's own `SKILL.md` atime
  (`st_atime_ns`, nanosecond-exact) immediately before that runtime runs,
  require the reported `last_used` to be derived from that value (never older,
  absent only where the fixture carries no evidence), and normalize the value
  before the byte comparison. `skills_list_last_used_json` additionally
  requires the value to be present, so the evidence contract is still checked.
- **`guard doctor` stays on the Go fallback, with a frozen corpus.** The Go
  oracle over seven scenarios is committed as `SEC-005` (`fixture-ready`, no
  Rust consumer). Measurement from this wave: the production output is not
  byte-stable even Go-to-Go, because it prints the building toolchain
  (`go1.27.1` locally versus `go1.26.7` under `run-go-oracle.sh`) and absolute
  temp-root paths. The oracle therefore writes `<root>` and `Go: <go>`
  placeholders; the toolchain line is an explicit accepted difference. A Rust
  module that hardcoded those lines was discarded — it is not a port.
  `make guard-doctor-oracle-check` verifies the fixture against the pinned Go
  revision and is wired into `rust-check`.
- **Not authorized by this checkpoint:** Go removal, pushing/publishing,
  release or product cutover, and any claim that the `fixture-ready` rows are
  green.

**Verified revision: `f2d18e3d`** on `migration/rust-continue-20260920`
(local only — not pushed, no PR). Five commits on top of `4ca840c2`:
`9e20d19a` (CFG-001 oracle), `0db1baf7` (guard-doctor fixture), `ca91ffdb`
(gate wiring + SEC-005 row), `3b4526e1` (#624 harness fix), `f2d18e3d`
(this checkpoint). The commit that adds this paragraph is docs-only, so the
verified revision stays `f2d18e3d`.

Verified at that revision: `make rust-check` → exit 0 (workspace fmt, clippy,
tests, `cargo-deny` advisories/bans/licenses/sources ok, and the oracle checks
`policy 0/50+30`, `xdg 0/256`, `guard 0/51`, `guard doctor 0/7`, `catalog 0/3`,
…); `make parity-smoke` → exit 0, 457/457 cases. The wave-1 worker branches
were cherry-picked, then removed; `.worktrees/` is back to the two wave-2
worktrees.

**Wave 2 dispatched** (`deleg_0befee8f`, 2 workers, both from `f2d18e3d`):
`migration/w2-db` in `.worktrees/w2-db` → `DB-001`/`DB-002`; `migration/w2-cli`
in `.worktrees/w2-cli` → `CLI-006`. Neither worker may touch the Makefile,
`scripts/rust-differential.py` or this ledger. Coordinator must verify their
commits and diffs, integrate in dependency order, and only then update rows.
Still open and untouched: `SEC-001`, `SEC-002`, `GCLI-PLAT-01`, `DIST-001`,
`DIST-002`, `GUI-001`.

## Resume checkpoint — 2026-09-20 (session `migration/rust-continue-20260920`)

Base revision: `48568aee` (main, CI green there including the macOS
`rust-check parity-smoke` job). Integration branch:
`migration/rust-continue-20260920`, local only — no push, no PR, no release.

- **Reconciled stale ledger claims.** Section "Latest integration update — guard
  CLI" below is historical. Its stated next action (reproduce
  `{"command":"open"}x`, repair the trailing-JSON diagnostic) is **done and
  verified on this revision**: the pinned Go oracle and the current native
  binary both emit
  `{"decision":"deny","reason":"decide: parse request: invalid character 'x' after top-level value"}`.
  `rust/symbrain-cli/src/guard_cli.rs` already carries the regression test
  `decide_preserves_go_trailing_json_diagnostic`. GCLI-F03 is closed by evidence,
  not by re-implementation. GCLI-PLAT-01 (non-Unix audit path) and Phase 8.5/8.6
  (`guard doctor` still returns `None` → Go fallback) remain open.
- **Fixed and verified: doctor vault-agent routing (#616).** `make rust-check`
  previously failed on any machine that has profiles, because
  `doctor_cli::requires_go_fallback` returned at the first `-vault-agent` and
  read the real XDG profile directory. The predicate now mirrors Go's
  `flag.FlagSet` scan (value consumption, `-h`/`-help`, undefined flag, missing
  value) and the profile precondition is injected
  (`requires_go_fallback_with`). Evidence: `cargo test -p symbrain-cli --lib`
  90 passed; `make rust-check parity-smoke` 458/458 twice; contract row
  `CLI-006B`.
- **Recorded external/new defect: #624.** Three atime-derived skills parity
  cases (`skills_list_managed_installs_json`, `skills_list_last_used_json`,
  `skills_list_target_flag_is_ignored`) flaked once and then passed 458/458 on
  two consecutive re-runs with no source change. `last_used`
  (`last_used_source: install_atime`) is the fixture file's atime, which the
  fixture pins and an ambient reader can bump afterwards, leaving the two
  runtimes reporting their own read clocks. Not a parity regression; the gate
  stays flaky until the fixture compares the atime captured per runtime.
- **Task order after this checkpoint** (dependency order, one writer each):
  1. `CFG-001` — XDG and legacy path precedence: source-bound Go oracle plus
     Rust comparison; row is `pending` while `rust/symbrain-core/src/xdg.rs` is
     already implemented.
  2. `#624` — make the atime-derived parity cases race-free (harness only).
  3. Phase 8.5/8.6 — `guard doctor` native port (the last `guard` verb on the Go
     fallback), then `SEC-001` secret-reference resolution and redaction.
  4. Phase 10 (`DB-001`, `DB-002`) and Phase 11 (`DIST-001`, `DIST-002`,
     `GUI-001`) remain untouched and are the largest open blocks.
- **Not authorized by this checkpoint:** Go removal, pushing/publishing,
  release or product cutover, and any claim that the `fixture-ready` rows are
  green.

## Latest integration update — guard CLI (historical, superseded above)

The previously test-only Unix `guard decide` adapter is now wired into this
local Rust integration candidate. Installed applications are unchanged. Other
guard verbs still use the Go fallback. Worker `sa-0-43ac72fd` has finished;
the coordinator owns this integration worktree and no Brain writer is active.
This update supersedes the earlier decoder checkpoint's pending-CLI statements.

Exact five-file delta, source-before/after hashes and reversible patch:
`/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-resume-evidence/brain-cli-integration-20260914T074809.707828Z/integration.json`.
The sibling `integrated-parity.json` records the rebuilt integration binary's
23/23 actual Go comparisons (response, exit, stderr, audit). Build, strict CLI
Clippy and workspace formatting passed after integration. Earlier worker tests
remain worker-source evidence until repeated on this candidate.

Independent review `deleg_5239fba0` approved the bounded Unix adapter and closed
GCLI-F01/F02 (help and ignored arguments). GCLI-BUILD-01 was resolved by rebuilding
both the reviewed worker binary and the integrated candidate. GCLI-F03 (trailing
JSON diagnostic) and GCLI-PLAT-01 (non-Unix audit unsupported) remain open; this is
not full guard/Brain parity or native/value/cutover acceptance.

Next concrete action: reproduce `{"command":"open"}x` against the pinned Go and
integrated Rust binaries, add it to the executable differential corpus, repair
the trailing-JSON diagnostic and repeat the affected candidate gates. Retain all
existing WIP, historical Go source and failed evidence. No PR or release.

## Resume checkpoint — 2026-09-14, guard decoder review

- Owner: migration coordinator. `sa-0-d06ca694` stopped without edits: runtime
  auto-isolated it in Vault, not Brain. Replacement `sa-0-43ac72fd`
  (`deleg_686b0cd6`) was dispatched after explicitly setting Brain cwd, with
  instructions to verify its own repository and restore the verified source
  overlay only into its clean private child worktree. Successful isolation and
  implementation remain unverified until the handoff; do not repeat the earlier
  claim that CLI integration is already underway. The integration worktree,
  lockfile, decoder and this register remain coordinator-owned. Worker lifetime
  is session-local, not durable after termination. Integration worktree:
  `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/symaira-brain/.worktrees/rust-integration-20260913`,
  branch `migration/rust-integration-20260913`; integration/Go oracle
  `0b585d52915a824664e1377d0a995dff3f5405cd` plus preserved WIP.
- Recovery manifest (main/origin SHAs, full owned file list, byte-verified overlay,
  staged/unstaged binary patches):
  `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-resume-evidence/20260914T072304.925477Z/brain/snapshot.json`.
  This retains the earlier Go audit/test changes, Raw-JSONL implementation,
  test-only adapter, fixture generators and this run's decoder changes. Do not
  reset, overwrite or reimplement that WIP.
- Current scope remains **partial**, not a completed vertical slice. The decoder
  now preserves duplicate null scalar/time fields and Go warning-slice reuse,
  folded JSON names, zero times and IPv6 zones. Audit serialization now follows
  the real Go command's `omitempty`; a former mirror-only test expectation was
  corrected against actual pinned-command output. Quoting reuses the existing
  `symbrain-core` implementation rather than a second formatter.
- Verified evidence:
  `/Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/rust-resume-evidence/brain-review-gate-20260914T072140.355630Z/report.json`.
  It binds source-before/after hashes, Rust/Cargo 1.98.0, native host, argv,
  cwd, individual exit codes and logs. Guard-core tests, strict Clippy and
  formatting passed. The new corpus compares 18 actual Go responses and audit
  records; the mutation test uses the real capture validator. Earlier failing
  run directories are retained, not relabeled as successes.
- Reproduce the retained-source Go capture without replacing it:
  `python3 guard/scripts/guard-decide-oracle/review_cases.py --check --output rust/symbrain-guard-core/tests/fixtures/external_decision_review.json`.
  The generator verifies the historical binary hash and full Git-source identity;
  its retained build lives in `target/migration-run/guard-decide-provenancefix`.
- Next executable gate:
  `python3 /Volumes/1TB_NVMe_SN850X/Dev/Symaira_Dev/Repos/docs/intern/brain-review-gate.py`.
  Then promote the existing test-only adapter into the local Rust CLI candidate
  and compare the actual binary against the pinned Go command. `guard` dispatch
  still returns `None` (Go fallback); installed applications are unchanged.
- Still open within Phase 5/guard-decide: full malformed JSON/time diagnostics
  (the time diagnostic helper is only lexically covered, not complete Go time
  parser parity), true binary dispatch, audit error/path/race and cancellation
  differential gates, independent delta review, native foreign-platform gates,
  remaining Brain/Browse/Guard contracts and final value/integration acceptance.
  No PR, merge, release or product cutover is authorized by this checkpoint.


> **For Hermes:** Execute this plan task-by-task with one implementation writer at a time and two-stage review (contract compliance, then code quality). Never remove the Go oracle before Phase 10.

**Goal:** Replace the Go implementation of `symbrain` with safe, idiomatic Rust while preserving CLI, MCP, SQLite, filesystem, Swift-client, security, and release contracts.

**Architecture:** A Rust host owns migrated commands and delegates unmigrated commands to a co-located Go oracle. Each vertical slice moves one externally testable capability behind the Rust dispatcher, adds Go↔Rust differential fixtures, and keeps both runtimes releasable. Domain crates stay under `rust/`; the SwiftUI apps remain Swift.

**Tech stack:** Rust 1.98/edition 2024, Serde, Tokio only at async boundaries, `rusqlite` with bundled SQLite/FTS5, `toml_edit`, official MCP Rust SDK where raw-frame parity permits it, `tracing` to stderr, proptest/fuzzing, cargo-nextest/audit/deny/llvm-cov.

---

## Global rules and gates

1. Keep Go buildable and tested until Phase 10 is complete.
2. Before each Rust port, freeze the Go behavior with a language-neutral fixture generated by the production Go binary or loader.
3. Run Rust and Go with identical argv, stdin, HOME/XDG roots, locale, timezone, and network fakes.
4. Compare exit status, stdout, stderr, file manifests/modes, SQLite state, and raw protocol frames in that order.
5. Treat every unexplained difference as a defect. Do not normalize it away.
6. Use `#![deny(unsafe_code)]`; any exception requires a documented invariant, focused tests, Miri, and review.
7. In MCP mode, stdout contains JSON-RPC frames only. All diagnostics go to stderr.
8. Do not change the Swift models to hide a Rust incompatibility. Rust must first satisfy the current JSON contract.
9. Do not commit, push, or cut over release assets without Daniel explicitly requesting those Git actions.

### Required fast gate after every task

```sh
make rust-check
make parity-smoke
make test
```

### Required phase gate

```sh
cargo nextest run --workspace --all-features
cargo test --workspace --doc --all-features
cargo hack check --workspace --each-feature --no-dev-deps
cargo llvm-cov nextest --workspace --all-features
cargo audit
cargo deny check
```

---

## Phase 0 — Baseline and reversible Rust host

**Status:** implemented on `feat/rust-migration`; retain until final review.

### Task 0.1: Measure and freeze the Go baseline
- Record Go files, packages, source lines, wall time, and memory in `MIGRATION-RUST.md`.
- Run `make test` before any Rust edit.
- Expected: all Go tests pass; baseline is reproducible.

### Task 0.2: Create the pinned Rust workspace
- Create `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, and `deny.toml`.
- Create `rust/symbrain-cli` with `#![deny(unsafe_code)]`.
- Expected: fmt, check, Clippy, tests, audit, and deny pass.

### Task 0.3: Add the dual-runtime dispatcher
- Rust owns no-argument help, help aliases, `version`, and `config path`.
- Unported commands invoke `SYMBRAIN_GO_BINARY` or `symbrain-go` with inherited stdio and exact exit status.
- Expected: a Rust invocation of `guard version` reaches the Go engine unchanged.

### Task 0.4: Add the neutral differential runner
- Maintain cases in `scripts/rust-differential.py` and contracts in `migration/contract-matrix.csv`.
- Build both binaries with the same version.
- Expected: all listed CLI cases match byte-for-byte.

### Task 0.5: Gate the migration in CI
- Add a pinned-action `rust-migration` job to `.github/workflows/ci.yml`.
- Expected: `make rust-check parity-smoke` is mandatory on every code PR.

---

## Phase 1 — Core, output, XDG, config, profiles, and policy

### Task 1.1: Extract `symbrain-core`
- Create `rust/symbrain-core/src/{lib.rs,exit.rs,output.rs,xdg.rs}`.
- Move exit codes, version payload, output format parsing, and XDG resolution out of `symbrain-cli`.
- Port tests from `internal/output`, `internal/paths`, and `internal/xdg`.
- Expected: CLI behavior unchanged; crate has no network/database dependency.

### Task 1.2: Port `config get`
- Add `toml_edit` and read `config.toml` through the production XDG path.
- Cover missing file, whole-file byte output, dotted keys, missing keys, malformed TOML, and extra arguments.
- Add differential cases with isolated fixture homes.
- Expected: exit codes and stdout/stderr match Go exactly.

### Task 1.3: Port `config set`
- Preserve bool/integer/string inference, dotted table creation, type-conflict errors, 0700 parent directory, 0600 file, and atomic replacement.
- Preserve unrelated keys and comments unless the Go oracle demonstrably does not.
- Compare recursive file manifest, mode, and parsed TOML.

### Task 1.4: Create `symbrain-policy`
- Port profile structs, TOML decoding, defaults, validation, server modes, allow/deny evaluation, annotations, and default-deny behavior from `internal/profile` and `internal/policy`.
- Use table-driven Go fixtures plus Rust parameterized tests for every mode matrix.

### Task 1.5: Port profile read/create commands
- Port `profile list`, `show`, and `add`; keep `profile remove` on the Go fallback until the harness-binding registry exists in Phase 6.
- Preserve JSON/table forms, ordering, templates, file modes, errors, and command-local flag placement.
- Run Swift model decoding tests against Go- and Rust-generated profile JSON.

### Phase 1 acceptance
- Every `config` row plus `profile list`, `profile show`, and `profile add` is green natively.
- Go and Rust profile JSON are semantically identical and stable in order where documented.
- Only `profile remove` may still invoke the Go fallback because its safety check depends on Phase 6 harness discovery.

---

## Phase 2 — Catalog, audit, patterns, and activity

### Task 2.1: Create `symbrain-catalog`
**Status:** implemented on `feat/rust-migration`.

- Port namespacing, collision detection, stable sorting, tool annotations, and routing metadata.
- Snapshot exact tool names/descriptions/schemas.

### Task 2.2: Create `symbrain-audit`
**Status:** implemented on `feat/rust-migration`; the frozen Go reader defect is tracked in #462.

- Port JSONL records, redaction, bounded backward tail, profile filters, degradation records, and file permissions.
- Compare bytes for fixed timestamps and semantic records for runtime timestamps.

### Task 2.3: Port `audit tail`
**Status:** implemented on `feat/rust-migration`; chained-envelope correction remains gated on #462.

- Cover small files, files larger than the reverse-scan chunk, missing files, malformed lines, `-n`, profile filters, and JSON/table output.

### Task 2.4: Port patterns and activity
**Status:** deterministic core implemented on `feat/rust-migration`; the DB-backed activity CLI remains on the Go fallback until the memory SQLite layer in Phase 10.

- Port episode recording, promotion thresholds, bounded token output, profile access, and redaction.
- Freeze Unicode truncation and ordering behavior with fixtures.

### Phase 2 acceptance
- Catalog snapshots, audit JSONL, activity fencing, and promoted-pattern decisions match Go.

---

## Phase 3 — MCP protocol and child broker

### Task 3.1: Create `symbrain-mcp`
**Status:** implemented on `feat/rust-migration`.

- Model JSON-RPC request/response/error types with exact snake_case and null/omission semantics.
- Implement both newline-delimited and `Content-Length` framing accepted by the Go server.
- Add malformed, oversized, partial, cancellation, and notification fixtures.

### Task 3.2: Fuzz protocol framing
**Status:** implemented on `feat/rust-migration` with Go-oracle seed drift checks.

- Add cargo-fuzz targets for frame decoding and JSON-RPC envelopes.
- Retain minimized crashes as regression fixtures.

### Task 3.3: Create `symbrain-broker`
**Status:** implemented on `feat/rust-migration`.

- Port child spawn/initialize/tools-list/tools-call, timeout handling, restart/backoff, health state, and stderr capture.
- Use process groups and guaranteed cleanup; verify no child survives cancellation.

### Task 3.4: Port the fake MCP child
**Status:** implemented on `feat/rust-migration`; Go and Rust broker scenarios share one oracle fixture.

- Replace or complement `internal/broker/testdata/fakemcp` with a language-neutral fixture executable/script.
- Run identical lifecycle scenarios against both brokers.

### Phase 3 acceptance
- Raw frames are byte-compatible where IDs and times are fixed.
- Crash/restart and shutdown leave zero child processes.
- Protocol fuzzing completes without crash or hang.

---

## Phase 4 — Managed binaries and passthrough

### Task 4.1: Create `symbrain-managed`
**Status:** implemented on `feat/rust-migration`; corrected unsafe Go archive selection is tracked in #465.

- Port embedded manifest parsing, platform/architecture mapping, release URL construction, SHA-256 verification, archive extraction, and atomic installation.
- Reject traversal, symlink escapes, unknown assets, and checksum mismatches.

### Task 4.2: Port `setup`
**Status:** implemented on `feat/rust-migration`; deterministic ordering, platform skips, and `v`-prefix repair matching were corrected in the Go oracle (#467). A pinned-checksum downgrade path was also closed in both runtimes (#466).

- Reuse HTTP fixtures and local release archives; never test against mutable live releases.
- Compare install/repair output, JSON reports, and installed file modes.

### Task 4.3: Port `vault` passthrough
**Status:** implemented on `feat/rust-migration`; validated by VLT-001/VLT-002 differential cases.

- Preserve opaque platform-native argv, inherited stdin/stdout/stderr and TTY behavior, child exit status/signal behavior, lookup order, and missing-binary diagnostics.
- Resolve in the same order as Go: configured `servers.vault.binary_path` (including its environment override), managed `~/.symaira/bin/symvault`, then executable `PATH` entries.
- Spawn with inherited stdio and mirror Go's signaled-child mapping (`ExitCode() == -1` becomes process status 255); do not alter the executable oracle's signal contract.
- Exercise only fake `symvault` scripts in the differential harness; no live vault or secrets are used.

### Phase 4 acceptance
- Every supported platform selects the same asset as Go.
- Security-negative archive fixtures fail before writing outside the destination.

---

## Phase 5 — MCP gateway and doctor

**Execution dependency:** Build the gateway domain, MCP loop, audit integration,
and doctor foundations in this phase. The native CLI now serves the proven
subset (gateway-owned tools plus configured stdio children), while full-profile
cutover remains blocked until the Skills, Usage, and Memory handler crates from
Phases 7, 9, and 10 are available. The Go gateway exposes those tools
in-process; replacing them with temporary child backends would neither prove
parity nor satisfy the no-fallback acceptance gate. Return to Phase 5 acceptance after those dependencies are green.

### Task 5.1: Create `symbrain-gateway`
- Port profile-bound catalog assembly, namespace rewriting, built-in tools, child/internal routing, errors, identity injection, and policy shaping.

### Task 5.2: Port `mcp` and deprecated `serve`
**Status:** native CLI cutover is implemented for the proven subset: profile loading,
foreign stdio children, the configured vault stdio child, gateway-owned tools,
raw JSON-RPC subprocess coverage, notification/EOF/error behavior, and Unix
signal/process-group cleanup. Embedded Memory, Skills, and Usage handlers are
not ported; enabled profiles fail closed with an explicit stderr blocker rather
than exposing placeholders or falling through to Go. Cross-platform lifecycle
coverage and final Phase 5 acceptance remain blocked on those handlers plus the
corrected corekit oracle.

- Preserve initialization capabilities, tools/list, tools/call, notifications, cancellation, EOF, and stderr-only deprecation warning.
- Keep the Go oracle available for other commands; `mcp` and `serve` never invoke it.

### Task 5.3: Port doctor checks
**Status:** native baseline implemented; isolated empty-state JSON/human output, flag behavior, and non-zero version probes are byte-exact. Subprocesses are hard-bounded and secret-bearing child stderr is redacted in Rust; Go correction is tracked by #473. Complex profile, handshake, malformed-config, `--fix`, and cross-platform fixtures remain before cutover approval.
- Port config/profile/server/harness/link/degradation checks and fix behavior.
- Preserve warning-vs-fatal classification and JSON schema.

### Task 5.4: Rebuild stdio hygiene gate
- Exercise cold start, list, invalid call, child crash, shutdown, and malformed input.
- Parse every stdout line/frame; reject any non-JSON-RPC byte.

### Phase 5 acceptance
- Official MCP conformance and raw Symaira differential fixtures pass.
- Rust `mcp` never invokes the Go fallback.

---

## Phase 6 — Harness registry, adapters, instructions, install, and sync

### Task 6.1: Create `symbrain-harness`
**Status:** H1–H5 implemented in the native `rust/symbrain-harness` workspace crate. The nine-entry registry, injected OS/environment path resolution, Entry/ServerInfo semantics, ordered JSON and TOML document operations, golden round trips, timestamped atomic backups, bounded unified diffs, read-only inventory, and profile bindings are covered by executable tests against the Go golden fixtures and production-derived registry contract. The Windows fallback path intentionally follows the injected target OS rather than reproducing the Go test helper defect tracked in #469; the correction is isolated to path resolution and remains fixture-ready for native Windows verification.
- Port the target registry and all config formats/paths/transports.
- Preserve unknown fields, ordering, comments where supported, and backups.

### Task 6.2: Port adapters
**Status:** implemented in `rust/symbrain-adapter`; the source-bound Go oracle pins production adapter/registry/instruction bytes and covers the four exact rendering adapters plus five explicit registry skips. Existing user bytes, malformed markers, CRLF/non-UTF-8, marker-bearing content, target paths, traversal and absolute-path rejection, capability-rooted parent confinement, nonblocking Unix reads, and durable atomic writes are tested. Unix mode bits, xattrs/POSIX ACL xattrs, and Windows owner/group/DACL preservation are implemented through retained handles; ordinary Windows writes do not request SACL privilege. Go sync emits target statuses before returning a non-zero failure decision.
CLI wiring and skills installation remain deferred.

- Port the actual Go registry contract: Claude, Claude Desktop, Codex, Cursor,
  OpenCode, Antigravity, Agents, Hermes, and OpenClaw. Do not invent adapters
  for Toolbox, Copilot, retired Gemini, Kiro, Zed, or Windsurf unless they first
  become supported Go contracts with fixtures.
- Maintain golden files per harness and platform.

### Task 6.3: Port managed instruction blocks
**Status:** implemented with capability-rooted source parents in Go and Rust. The source-bound Go oracle and `rust/symbrain-instructions` share a 1 MiB per-file / 1.5 MiB merged-source limit, regular-file/no-follow reads, component-wise trusted-root/source-parent walks on Unix and Windows, XDG source-path cases, a true prefix-code sentinel escape with collision proofs, and four deterministic source-error verdicts. Negative source diagnostics now compare exact normalized bytes, including the Go `instructions: read <path>:` wrapper; fixture normalization removes only isolated temporary roots and uses slash separators, without weakening redaction. Unix final opens are nonblocking and FIFO/symlink/late-parent races are covered. Go and Rust atomic replacement preserve complete Unix mode bits and readable xattrs/POSIX ACL xattrs; macOS's OS-owned `com.apple.provenance` is explicitly excluded. Windows adapter replacement preserves owner/group/DACL without requesting SACL privilege; unsupported post-rename directory flushes are nonfatal after file data is flushed.
- Preserve all bytes outside markers; guarantee idempotency and newline behavior.
- Add proptest round trips.

### Task 6.4: Port harness/install/uninstall/sync commands
**Status:** Phase 6.3A install/uninstall is native and green for the exercised macOS/Linux paths, with Windows target compilation verified but native Windows runtime still unclaimed. Trusted roots are opened component-by-component from filesystem anchors without following symlink/reparse ancestors; config reads, JSON/TOML parsing, backup names, temporary names, replacement, rollback, and cleanup are bounded and capability-rooted. A source-bound Go oracle runs the production CLI in isolated HOME/XDG/project roots and freezes entry bytes/semantics, malformed-before-side-effect refusal, dry-run, profile defaults and environment override, Claude project scope, superseded migration, foreign-name protection, no-op cases, collision-safe backup shape/content/modes, rollback on failure, symlink rejection, and parent/file modes. Native Rust routing uses `symbrain-harness` capability paths; unrelated commands retain the Go fallback. Instruction source and adapter reads now retain no-follow root/parent capabilities and use bounded regular-file checks with nonblocking Unix final opens; Go and Rust replacement preserve Unix mode/xattr/POSIX-ACL metadata and Windows owner/group/DACL metadata without requesting ordinary-write SACL privilege. Unsupported Windows directory flushes are nonfatal after the rename has committed and file data has been flushed. Skills sync remains deferred.

Source-bound profile removal is now implemented in `rust/symbrain-cli` and
covered by a 20-case Go oracle. Binding scans fail closed on unreadable,
malformed, symlinked, FIFO, and other special configuration files; confirmation
input is bounded, and `--force` is the explicit override. The profile target
itself is deleted through retained no-follow capabilities, including protection
against a symlinked `profiles` directory. Native Windows runtime remains
unclaimed; Windows target compilation is verified in the matrix.
- Cover global/project scope, dry run, superseded server migration, backups, health probes, partial failures, and JSON summaries.
- Keep the profile remove contract covered by its native harness registry, source-bound oracle, and platform-specific safety tests.

### Phase 6 acceptance
- Sync twice produces a zero-byte diff.
- Every harness golden and Swift `SyncSummary`/`HarnessStatus` decoder passes.

---

## Phase 7 — Skills engine

### Task 7.1A: Create `symbrain-skills` model and variants
**Status:** fixture-ready on `feat/rust-migration`; the Rust crate is implemented and tested against the tracked Go render fixtures, with a deterministic Go loader oracle and Rust source-bound comparison. The complete slice remains fixture-ready rather than green because target rendering is intentionally deferred to Task 7.2.

- Port `SKILL.md` frontmatter and `symskills.toml` manifest parsing, bundle/resource loading, root-contained path checks, validation, render-blocking classification, and deterministic markdown/overlay discovery.
- Port flat `block`, `only`, and `except` markers, target-specific block overrides, term substitution, unscoped mention detection, malformed-marker diagnostics, and cross-file uniqueness/reference checks.
- Keep config, profiles, rendering, installation, VCS, CLI, and MCP behavior for later tasks.

### Task 7.1: Create `symbrain-skills` domain model
- Port config, metadata, frontmatter, variants, profile resolution, discovery, and deterministic ordering.

### Task 7.2: Port rendering
- Freeze every existing `internal/skills/render/testdata` artifact from Go.
- Match generated bytes, permissions, and symlink behavior.

### Task 7.3: Port install/status/restore/VCS
- Preserve three-way drift classification, unmanaged protection, conflict handling, operation log, and Git behavior.

### Task 7.4: Port skills CLI and MCP tools
- Port list/status/targets/log/sync/doctor and all skills MCP schemas/handlers.

### Phase 7 acceptance
- All skill goldens are byte-identical.
- Rust never overwrites harness-changed, conflicting, orphaned, or unmanaged content contrary to Go behavior.

---

## Phase 8 — Guard security module

Go remains the production enforcement path until the full Guard-specific matrix is green. Brain capability shaping must not absorb any per-call approval or risk behavior during the port.

### Task 8.0: Freeze Guard contracts with a source-bound oracle
- Add deterministic fixtures for model validation, policy buckets and precedence, sequence streams, config, capability vectors, audit formats, grants, discovery, spawn, and each CLI command.
- Inject clocks, identifiers, randomness, HOME/XDG roots, and fake network responses. Correct or explicitly record nondeterministic Go behavior before generating fixtures.

### Task 8.1: Port deterministic model and static policy
- Port request/decision models, validation, fail-closed responses, static rule buckets, merge, scope ceilings, marginal capability, risk classification, and deterministic reasons.
- Keep grants, sequence state, capabilities, audit, discovery, and CLI dispatch out of this slice.

### Task 8.2: Port sequence detector and config
- Port bounded repetition/search state with deterministic tie-breaking.
- Port TOML/XDG/defaults/validation and spawn allowlist configuration.

### Task 8.3: Port capability tokens and audit compatibility
- Verify fixed HKDF/HMAC/base64 vectors and expiry boundaries.
- Match both the selected Guard raw-line format and the canonical chained-envelope/anchor contract before consolidation.

### Task 8.4: Port grants, approval, and proposals
- Match expiry, ordering, tombstones, permissions, atomic replacement, human-only durable mutations, and audit sink requirements.

### Task 8.5: Port discovery, spawn, and output
- Preserve corekit-backed client ordering, path/platform behavior, environment redaction, absolute-path/argv gating, and stdout/stderr semantics.

### Task 8.6: Port guard commands and cut over
- Port decide/doctor/grants/scan/version with exact stdin/stdout/stderr and exit semantics.
- Switch the dispatcher only after every Guard row is green; until then Go remains the production path.

### Phase 8 acceptance
- Go and Rust produce identical decisions for the full policy corpus.
- Existing Go audit chains remain verifiable and Rust chains are accepted by Go.

---

## Phase 9 — Usage providers

### Task 9.1: Create `symbrain-usage` common model
- Port provider status, percentages, reset times, errors, ordering, and bounded concurrency.

### Task 9.2: Port file/local-server providers
- Port Antigravity/OpenUsage, Codex, OpenCode, Kimi, Moonshot, Nous, and local JSON/plist readers using fixture files.

### Task 9.3: Port HTTP providers
- Port Claude, Copilot, Cursor, OpenRouter and any remaining APIs using recorded mock servers, explicit timeouts, and redacted errors.

### Task 9.4: Port Darwin Keychain lookup
- Shell out to `/usr/bin/security` with bounded timeout and no secret output. Add macOS-native integration tests that never touch Daniel's real credentials.

### Phase 9 acceptance
- Sanitized provider fixtures produce matching JSON; unsupported/unconfigured states degrade identically.

---

## Phase 10 — Memory core and SQLite

### Task 10.1: Freeze the database contract
- Generate a Go v0.11.0 fixture for every schema version and representative live shape without personal data.
- Record schema, indexes, triggers, pragmas, NULL/time encodings, and ordered query snapshots.

### Task 10.2: Create `symbrain-memory` storage layer
- Use `rusqlite` with bundled FTS5; apply the exact migration sequence and transaction boundaries.
- Match single-connection behavior, busy timeout, WAL, foreign keys, secure delete, locking, rollback, and interrupted migration semantics.

### Task 10.3: Port memory CRUD, entities, temporal data, and rules
- Preserve IDs, timestamps, scopes/kinds, evidence, staged state, soft/hard deletion, entity links, and ordering.

### Task 10.4: Port vector encoding and retrieval math
- Preserve 768-bit LSB-first little-endian sign encoding (`<= 0` means bit 1), Hamming distance, stable tie ordering, FTS tokenization, RRF `k=60`, Sparsemax, and spreading activation.
- Require byte equality for vectors and rank equality; float tolerance at most `1e-6` where bytes are not contractual.

### Task 10.5: Port importers and discovery
- Port each importer as its own reviewed task: Aider, Calendar, Claude Code, Codex, Codex Memory, Curated Memory, Email, Git, GitHub, Hermes, Memory Tool, Obsidian, OpenCode, Paperless, Shell History.
- Give every importer positive, empty, malformed, duplicate, and Unicode fixtures.

### Task 10.6: Port LLM, extraction, consolidation, context, aging, and working memory
- Use fake providers for deterministic tests; preserve prompts/output parsers and failure fallback.

### Task 10.7: Port memory CLI, MCP tools, sync, and HTTP server
- Port list/search/set/delete/rules/query-log/sync/serve and all memory MCP schemas.
- Preserve JWT/secret resolution (`symvault://`, deprecated `vault://`, env fallback), loopback binding, request limits, and encrypted relay compatibility.

### Task 10.8: Port the TUI
- Reproduce keyboard actions and semantic states with `ratatui`; visual pixel parity is not required unless documented, but all data mutations and exits are.

### Phase 10 acceptance
- Rust opens and migrates every Go database fixture.
- Go opens Rust-written databases and passes integrity checks.
- Search candidate sets/ranks match; encrypted artifacts round-trip cross-runtime.
- Memory CLI/MCP/HTTP contracts are green without fallback.

---

## Phase 11 — Swift clients and cross-platform release

### Task 11.1: Run Swift contract tests against Rust
- Point `SymBrainCoreTests` at the Rust binary without weakening models or tests.
- Verify macOS app workflows; keep iOS read-only behavior unchanged.

### Task 11.2: Add native target CI
- Run Rust on Ubuntu, macOS, and Windows; use native runners for runtime behavior.
- Add scheduled Miri, fuzz, mutation, coverage, audit, deny, and unsafe-inventory jobs.

### Task 11.3: Build hybrid release artifacts
- During migration, package Rust `symbrain` plus platform-matched Go fallback while presenting one supported command.
- Preserve archive names, checksums, Cosign signatures, CycloneDX SBOMs, Homebrew behavior, GUI DMG embedding, signing, and notarization.

### Task 11.4: Benchmark release candidates
- Compare startup, steady RSS, command latency, MCP throughput, database size, search latency, and final archive size against the baseline.
- Publish only measured gains; record regressions honestly.

### Phase 11 acceptance
- All supported native platforms and Swift tests pass.
- Release artifact manifests match the established contract or an explicitly versioned migration.

---

## Phase 12 — Final cutover and Go removal

### Task 12.1: Prove zero fallback use
- Instrument test builds to fail if any command attempts the Go bridge.
- Run the complete contract matrix, including negative cases and side effects.

### Task 12.2: Ship a reversible Rust prerelease
- Keep the last known-good Go release/tag and documented rollback path.
- Run real harness smoke tests with sanitized user profiles and databases.

### Task 12.3: Remove the fallback bridge
- Remove Go embedding/packaging only after one prerelease cycle has no unexplained parity defects.
- Keep Go source in the repository for one stable Rust release if maintenance cost permits.

### Task 12.4: Remove Go build infrastructure
- In a separate reviewed change, delete Go source/modules, GoReleaser Go builds, and Go CI.
- Update AGENTS.md, README, release workflows, Homebrew, architecture docs, and contributor instructions for Rust.

### Task 12.5: Final release verification
- Rebuild from a clean checkout on all native platforms.
- Verify binary identity, archives, checksums, signatures, SBOMs, Homebrew install/test, macOS signing/notarization, Swift app launch, MCP conformance, database rollback, and documented recovery.

### Completion definition

The migration is complete only when every row in `migration/contract-matrix.csv` is green, no command invokes Go, the supported release matrix passes, Swift clients work unchanged at their public boundary, and the Rust release has a verified rollback to the last Go release.
