# Independent review: standalone Guard increment (#770)

Reviewer: independent read-only Codex agent `review_pr800`.
Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Reviewed clean source head: `42b580479a2e075dc5d95a633ec489e25c671878`.
Reviewed clean publication head: `4a9b9a6555d25e584ed34d4f7a7c381036ca766c`.
Worktree: `/workspace/symaira-guard770`.
Disposition: **no actionable findings in this scoped increment; suitable for acceptance after fresh exact-head native Linux/macOS/Windows CI and protected checks pass**. #770 remains open. The increment restores an independently usable source-buildable executable, not complete migration, proxy functionality, distribution or zero-unported acceptance.

## Inspected scope and source identity

Read the root and Guard `AGENTS.md`, all changed production files and manifests/lock changes, every new standalone/shared handler and doctor fragment, moved grants/store/scan parser, changed and new tests, supplemental Go main, entire process harness, three native workflow additions, README changes, ADR, migration inventory/ledger and tracked receipts. Read the actual Go version, scan and doctor commands and the existing safe raw JSONL appender; checked the actual reachable dispatcher inventory. No frozen production Go or old fixture changes occur in this patch.

The publication successor changes only `migration/evidence/guard-standalone-770/linux-process-report.json`. All 17 recorded new-crate source hashes and three manifest/lock hashes match both source head 42b5804 and publication head 4a9b9a6. The fresh independent replay bound publication head with `candidate_dirty: false`, and verified **1,217** original Go/module source inputs against immutable revision `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`.

Confirmed `guard_grants.rs`, `guard_grants_store.rs` and `guard_scan_config.rs` are byte-identical moves from E8. Comparing E8's scan module with the new module shows only extraction of its existing test body. Reconstructed the split doctor implementation and reviewed the diff: configuration, audit, discovery, allowlist/secret findings and report bodies retain existing logic; module visibility/imports, comments and formatting changed. Diagnostic gates are preserved. Reconstruction evidence: `/tmp/symaira-guard770-doctor-reconstructed.diff`.

## Correctness and product boundaries

The new `symguard-cli` crate owns the shared Guard command implementation. Brain is a two-line compatibility re-export and retains its previous migration fallback for unsupported Doctor states. The standalone entrypoint has no Go or Brain executor and turns an unsupported Doctor state into exit 1, empty stdout and an explicit stderr diagnostic. Thus it cannot silently produce a healthy-looking partial report or invoke an absent legacy executable.

The normal dependency graph includes the existing Guard policy/audit kernel, safe raw JSONL writer and reusable core primitives. It excludes `symbrain-cli`, gateway, broker, memory, exposure policy and usage. The small pure `symbrain-core` dependency supplies paths, version, exit and formatting primitives, not a Brain gateway or per-call approval authority. No separate credential store, master key, Room authority, provider endpoint or child MCP launch is introduced. Brain/Guard and Room/Guard boundaries remain intact.

The actual frozen Go source exposes five command packages (version, doctor, decide, grants and scan) through Brain, with no standalone Go main. The supplemental process main wires these unchanged exported handlers to the new standalone presentation. Proxy/spawn/approval/proposal/sequence/update libraries are not invented as reachable CLI verbs. Standalone top-level help is intentionally standalone-specific; subcommands retain their shipped Guard spelling and streams.

The substantive version correction is justified by real mismatches: JSON now uses the actual Go compact serialization and two newline bytes; both human/JSON version writes propagate failure. Rust SDK labels are truthful explicit runtime differences, rather than fabricated Go-version claims. Decide retains bounded input, actual Guard-domain policy, deny on parse/read/audit failure, private append-only audit state and the existing no-follow directory/target protections. Grants retain the existing parsing, active ordering, persistent-scope filtering and atomic private-file replacement. Scan redacts every environment value and keeps deterministic inventory order and existing parser/error behavior.

Production files remain under 400 lines. Splitting the prior oversized doctor and moving Guard handlers out of Brain improves ownership and reviewability without duplicating state or policies. No new expensive algorithm, unbounded subprocess or network operation appears. Existing discovery/config file size and malformed-output/error compatibility boundaries are not silently represented as completed.

## Independent execution and supplementary probes

Used `set -euo pipefail`, `umask 022`, the session subreaper, owned target directory, Rust 1.98 and pinned Go 1.26.7. Re-ran the complete focused process gate independently at clean publication head. Receipt: `/tmp/symaira-guard770-independent-process-report.json`; log: `/tmp/symaira-guard770-independent-gate.log`.

* **80** distinct real native/frozen-Go process cases executed. **76** matched complete stdout/stderr/exit and regular-file modes/content after owned-path normalization and explicitly validated SDK/audit runtime fields.
* **Three** unported Doctor states actually executed and failed closed with exit 1 and no native stdout. They remain labeled remaining-port states, not matches.
* **One** inherited audit-open failure actually returned deny in both implementations while retaining the differing complete reason bytes. It remains a diagnostic deviation, not parity.
* Native executable SHA256: `cc90eaeba6836865096ded0f7378fe31a954718fb6ab69833682008ea8669311`. Independently built Go executable SHA256: `3a100de0e54693a6233cb7ac981f03e8a44a41db7dfd24d1e313a417631ff34c`.
* Re-executed the current 18 compiled test executables corresponding to the source-bound Guard/audit/kernel and Brain compatibility gate: **116 parent tests passed, zero failed/ignored**. A filtered subprocess test summary is counted only within its parent, not added again. Receipt with executable hashes: `/tmp/symaira-guard770-independent-tests.json`; complete outputs: `/tmp/symaira-guard770-independent-tests.log`.

Supplementary actual standalone probes used disposable private roots, empty PATH and an explicitly absent Go fallback. All passed; receipt: `/tmp/symaira-guard770-independent-private-state.json`:

* Two successful decides append two complete records while preserving the first record, use directory mode 0700/file mode 0600 and create no Brain data namespace.
* A symlink audit target returns deny and leaves the owned sentinel unchanged; a FIFO target returns deny without blocking.
* An oversized request returns deny. A malformed discovery configuration makes Doctor fail closed with empty stdout and the explicit unsupported diagnostic.
* Device-scoped grant revocation writes a private 0600 store, preserves the unrelated device grant and is reflected by a second independent list process.
* Two independently applied comparator mutations (changed version stdout and changed empty-command exit) were rejected. These temporary comparator probes do not modify frozen fixtures or claim additional production process cases.

The independently observed native binary hash matches the author's clean source proof. Checked the recorded validation logs and their SHA256 values: author's 98 Guard/audit/kernel plus 18 Brain tests equal 116, with zero failures/ignored; strict all-target/all-feature Clippy completed successfully. Formatting, actionlint and script syntax are recorded as passed in the source-bound receipt; this review does not claim an independent repeat of those static checks.

## Evidence integrity, portability and remaining acceptance

The harness clears operator environment, provides owned HOME/USERPROFILE/XDG roots and empty PATH, and never reads real credentials or starts a real server. Both implementations receive the same synthetic config/grants/request states. Audit IDs/timestamps are checked against real process execution windows and against each other before replacing only those dynamic field values; original raw output/state bytes remain in the receipt. Complete case cardinality and uniqueness are asserted. Neither skipped unsupported states nor the excluded platform-specific broken-output fixture inflate the match count.

The workflow invokes the named process gate on native Linux, macOS and Windows, handles Windows `.exe` paths through `cygpath`, uploads the receipt on failure/success, and rejects a missing artifact. Errors before the final report is constructed can leave no report; the runner/upload then fail, rather than producing a fabricated passing receipt. Native exact-head three-OS results remain required and are not substituted by this Linux review.

Keep the documented acceptance work open: complex Doctor TOML/type/validation and audit-anchor shape diagnostics, malformed discovery/config error boundaries, inherited audit-error spelling, platform-specific broken output, complete native acceptance and release/signing/distribution. No zero-unported, complete CLI migration or shipped standalone release claim follows from the scoped 80-case gate. No repository source edit, commit, push, PR state change or merge was made by this reviewer.
