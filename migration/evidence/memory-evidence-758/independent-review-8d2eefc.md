# Independent review: native memory evidence increment

Reviewer: read-only independent Codex agent `review_pr800`.
Base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`.
Initial reviewed head: `acc3bcf8011b5e50f25f4dce873a1f1e6a548067`.
Final reviewed production/test/script head: `8d2eefc5c02a77de7eccddb486f5b227e91b079d`.
Verified publication head: `5423e8225c4f350cb363af0dfa4e8480c4e2597a`; its three-file successor diff changes evidence only, and all 38 reviewed source hashes remain identical.
Worktree: `/workspace/symaira-memory758`.
Disposition: **no remaining actionable findings; the scoped increment is suitable for acceptance after fresh exact-head native Linux/macOS/Windows CI and protected checks pass**. This review does not certify completion of #758 or release-dependent #649. Evidence-only successor commits may carry this review if all reviewed source hashes remain identical.

## Resolved findings

### P2: Deferred schema transaction regressed simultaneous public opens

The original implementation used `Connection::transaction()` after configuring SQLite, then read schema/columns before publishing migration entries. Concurrent connections could commit between that deferred read snapshot and its attempted write upgrade. SQLite returned `SQLITE_BUSY_SNAPSHOT` (517), or `DatabaseBusy` (5), immediately despite the five-second busy timeout. In real applications, simultaneous startup or commands could therefore fail opening an otherwise valid store.

An independently compiled temporary probe used only the public `Store::open`, `get` and `set` APIs. Each trial seeded a legacy disk database with all migration entries already applied, all five #649 columns absent and an existing Unicode row. Two initial reopen/write operations succeeded, followed by eight barrier-synchronized independent connections. The original candidate **failed five of five trials**, while the E8 baseline memory implementation **passed five of five** with the identical probe. Literal stderr, executable/rlib hashes, complete baseline source hashes and original-row preservation are retained in `/tmp/symaira-memory758-independent-concurrent-open-regression.json`; the implementation agent preserved that evidence in the repository.

The correction uses `transaction_with_behavior(TransactionBehavior::Immediate)` before schema inspection. It reserves the writer before a snapshot is taken, so competing openers wait within the existing timeout while schema creation, additive repair, indexes and bookkeeping still share rollback. I inspected the corrected production line, its rationale and the real public-opener regression test. The independently linked corrected library **passed all five replacement trials**, including 40 concurrent public open/write operations, two initial reopens per trial, repair of all five columns and exact preservation of every previously existing column value. Every trial retained all eleven expected memory rows. Correction receipt: `/tmp/symaira-memory758-independent-fixed-concurrency.json`.

### Native-platform failure-control formatting

The strengthened control encoder round-tripped the original Go fixture in memory, but `Path.write_text` would translate newlines on Windows when writing mutation fixtures. That could introduce an incidental raw-byte mismatch alongside the intended semantic mutation. The implementation now writes explicit UTF-8 bytes for both changed-status and removed-case fixtures. I reviewed that change and independently reran all three actual failing replay processes. The original fixture round-trip assertion remains; each control fails with its required diagnostic.

## Inspected scope

Read root `AGENTS.md`, migration narrative and ledger changes, public ADR, all production additions and refactors: `evidence/mod.rs`, `evidence/fuzzy.rs`, `evidence_store.rs`, `migration.rs`, `store.rs`, `write.rs` and `lib.rs`. Read the evidence-store unit tests, evidence integration tests, additive fixture, native workflow and all four supplemental runner/generator/control scripts plus their README. Read the complete pinned CoreKit v0.17.0 `evidencekit.go` and Brain's production `internal/memory/db/evidence.go` to establish semantics rather than infer them from Rust tests.

The frozen algorithm matches the implementation: exact alignment precedes normalized Unicode whitespace, then weighted-LCS fuzzy windows; punctuation remains part of whitespace-delimited tokens; token edit distances count Unicode runes. Equal scores retain the first shorter window and earlier byte position. The explicit whitespace set matches Go's stable `unicode.IsSpace` table. Normalization maps first collapsed whitespace runes back to original UTF-8 byte offsets; `char_start`/`char_end` remain byte coordinates. Validation keeps the Go error priority, rejects fuzzy/unmatched by default, and intentionally preserves zero-length and future-status acceptance.

JSONL fields, omission rules, sorted attribute keys, Unicode/HTML escaping and byte hash match actual Go observations. New evidence persistence uses bound SQL values, UUIDv4-shaped IDs, one creation timestamp per batch and the same oldest-first query. It preserves strict rejection/skipping, foreign-key failure, cascading deletion and transaction-owned save/reparent semantics. The refactored memory-write body retains its previous redaction, hashes, embeddings, identifiers and stored fields; no CLI fallback route or profile capability changes. The repaired migration inspects actual columns instead of trusting migration names, rolls all DDL/index/bookkeeping changes back on failure, and leaves journal configuration outside the transaction as SQLite requires.

Reviewed correctness, security boundary, performance/resource behavior and portability. No new credential, browser, external-provider, unsafe-code or authority exposure was introduced. Fuzzy alignment remains a compatibility helper requiring an ingestion owner's input budget; no newly exposed endpoint invokes it, and full ingestion/CLI cutover is explicitly deferred. Production modules remain under 400 lines. The workflow uses pinned checkout/setup-Go/upload actions, Go 1.26.7 and repository-pinned Rust 1.98; three actual native OS jobs preserve exact-head SDK/source receipts and fail closed on replay/controls/tests/lint. Their execution is still required; this Linux review cannot substitute for native CI.

## Independent verification

* Checked the clean source-bound receipt for `8d2eefc5c02a77de7eccddb486f5b227e91b079d`, every one of its 38 candidate source hashes and the matching compiled memory rlib SHA256 `373f2f945aa2cfeeaf0ff5368bce3cf9011bfc06187de626ab44ef8ed1226ba3`.
* Independently replayed immutable Brain `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` with Go 1.26.7. Verified pinned CoreKit source SHA256 `8316336a261e6ab9ff8c525bdf8cae09c052a71c938d93204870b1b4d1808e54`: 32 alignment pairs, 48 validation boundaries, three byte-exact JSONL records/hash and four production Go database tests passed. Receipt: `/tmp/symaira-memory758-independent-fixed-oracle.json`.
* Independently executed all three strengthened missing/mutated/truncated replay controls: nonzero exits and required reasons observed. Receipt: `/tmp/symaira-memory758-independent-fixed-controls.json`.
* Independently executed the current compiled memory tests without Cargo mutation or build-lock contention: **42 passed, zero failed/ignored** (31 unit, four Go-derived evidence integration, seven store contracts). This includes the real public rollback regression and ten rounds of eight public openers/writes. Log: `/tmp/symaira-memory758-independent-native-tests.log`.
* Independently reproduced the original concurrency failure against the actual baseline and verified the corrected five-trial probe as detailed above. Temporary probes and disk databases stayed outside the repository and operator HOME.
* Inspected the implementation agent's complete clean-head source receipt and test/static-check logs: 333 affected/consumer tests passed, zero failures/ignored, with strict Clippy, formatting and actionlint passed. Those broader consumer tests were not claimed as independently rerun.

Subprocess checks used the session subreaper, `set -euo pipefail` and `umask 022`. I made no repository source edit, commit, push, PR state change or merge. The implementation agent authored the corrections; I independently reproduced their regression and checked the replacement artifacts.

Full memory CLI flags/configuration/governed writes, Go-compatible JSONL decoding, `memory serve` and sync remain open in #758 and dependent issues. #649 retains its shipped-release and Doctor-diagnostic conditions. Keep these issues open after merging this coherent increment.
