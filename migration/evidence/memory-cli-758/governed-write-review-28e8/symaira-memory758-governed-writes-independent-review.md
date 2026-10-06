# Independent governed Memory CLI write/delete review

Disposition: **REQUEST CHANGES — two confirmed P2 groups**. The standard corpus passes independently; neither additional failure is fixed or waived. No candidate source, fixture, Go code, branch, pull request or issue was modified by this reviewer.

Reviewed immutable publication `28e8e7ae1a1eb05b3ba1c83a56bbebf1a3572a24` in `/workspace/symaira-memory758-writes`, clean source `14d24144df9fdf7e55db25b0ba64b1dcfa587468`. Actual integrated main is `31de72294521701fc4b1a0bce39f03cc34d72e7e`; the immediate normally integrated Memory CLI parent is `9c0ed0650e4163a60fe63f9c651a6efae3c2e6ad`. Main is an ancestor. The publication's 40 changes relative to source are documentation/evidence only; production/scripts/workflow bytes are identical. Frozen oracle is `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, actual Go SDK 1.26.7 and CoreKit v0.17.0.

## Findings

### P2 — require a changed row before reporting governance success

`rust/symbrain-memory/src/cli_write.rs:99` ignores the count returned by both the kind update and the staged-review update (lines 99–107). Frozen Go `SetMemoryKind` and `SetMemoryReviewStatus` each reject `RowsAffected()==0` with `memory not found: <id>` and stop the service sequence. An existing store can legitimately produce zero changes through a callback or disappearance after insertion; routing does not delegate these admitted stores.

Three independent actual Go/native process pairs confirm the difference with unchanged binaries, private HOME/XDG, empty PATH, absent Go fallback, a real owned embedding HTTP server and the same backed-up Go-created database:

* A `BEFORE UPDATE OF kind` trigger containing `SELECT RAISE(IGNORE)` makes Go return 1 with empty stdout. Native returns 0 and claims kind `reference`, although the persisted kind stays empty; native also proceeds to staging whereas Go stops before it.
* A `BEFORE UPDATE OF review_status` trigger containing `SELECT RAISE(IGNORE)` makes Go return 1. Native returns 0 and claims `staged:true`, while the actual row remains `approved` and therefore live.
* An `AFTER INSERT ON audit_log WHEN new.action='set'` callback removes the inserted memory. Go reports missing memory; native returns a successful ID for a row that does not exist.

Both databases pass real FTS integrity checks in all three pairs. These are valid state callbacks, distinct from the author's preserved rejected FTS fixture. The complete literal commands, SQLite trigger definitions, stdout/stderr, tables, blobs and binary hashes are in `/tmp/symaira-memory758-governed-independent-extra.{py,json,log}`. Enforce Go's zero-row checks at both updates and preserve its stop order; add these actual Go cases to the permanent state gate. A broad transaction around the entire service is not the correction: Go itself persists the earlier save/audit before a later governance failure.

### P2 — propagate output failure on newly admitted native Set paths

`rust/symbrain-cli/src/memory_cli/write.rs:83` and line 98 discard the JSON/table output write result, then line 101 returns success. This pattern existed in the previous native adapter, but this patch expands admission to metadata, configured, nondefault-author and alias writes that previously delegated Go. Two independent cases specifically use newly eligible nonempty `--metadata '{"fixture":"value"}'`, `--staged`, valid existing store, and JSON/table output sent to the real `/dev/full` sink. Parent `9c0ed065` explicitly delegated every nonempty metadata Set, so this newly exposes a formerly Go-owned error path.

Actual Go returns 1 and `symbrain memory set: format output: write /dev/stdout: no space left on device\n`; native returns 0 with empty stderr. The memory write itself completes on both sides and FTS integrity passes. A caller thus receives a successful process exit even though its requested result could not be emitted. Evidence: `/tmp/symaira-memory758-governed-independent-output-extra.{py,json,log}`. Check rendering errors and preserve the CLI error/exit contract without rolling back the already completed store operation; retain real output-failure cases plus a portable failing-writer contract for native platforms.

The first extra probe also confirms four inherited generic Set/Delete JSON/table `/dev/full` differences. The second finding is scoped to this patch's newly admitted nonempty-metadata Set route; it does not mislabel the pre-existing Delete output implementation as newly introduced.

## Independent verification

The author explicitly released the target before any executable was used. No Cargo build/check/Clippy invocation or target regeneration was performed. Existing source-bound test executables and the actual CLI were reused under a Linux subreaper, umask 022, with pinned Go SDK PATH for harness helper builds.

* Fresh complete governed gate: **60/60 native Set pairs, 16/16 native Delete pairs, 13/13 actual delegated-Go boundary observations**. Full application rows/columns/blobs, ID bindings, temporal ordering, logical FTS plus actual integrity, embedding requests, audits and sync sequence remain checked.
* Fresh identity/audit/exit mutants: **3/3 detected**, each executes the real candidate and fails at the intended semantic assertion. The audit mutant changes only the returned primary memory's set audit; unrelated seeded audits remain untouched.
* Fresh complete baseline: **590/590**, reflected 86-field config schema, literal output/exit and seeded database state checks; fallback absent.
* Fresh baseline stdout/exit mutants: **2/2**, each rejects **all 32 actual seeded-read cases** for the intended assertion.
* Source-bound existing Rust test binaries: **161 distinct tests**, 0 failed/ignored: CLI library 117, Memory unit 32, immutable Go evidence fixture 4, store contracts 7, complete Activity fixture 1. An initial focused 12 CLI tests was also run; it is not added to 161. The Activity test executes the full existing fixture internally; it is one Rust test, not 265 separate Rust tests. This is not a fresh claim of the older historical 333-test gate.
* Fresh `cargo fmt --all -- --check` and actionlint for the changed workflow pass. Source/docs diff whitespace check passes. An unfiltered Git whitespace check reports trailing blank lines only in three byte-preserved raw test logs; those logs were retained unchanged, not edited to cosmetically alter evidence.
* **159/159 current candidate-source hashes**, **1215/1215 frozen Go source hashes**, **36/36 clean-source manifest hashes**, **9/9 executable/SDK hashes**, and **36/36 stored evidence files** verified, including decompressed original-byte hashes. Publication is still clean. The 36 source manifest entries bind source14d; three final public docs legitimately differ in the evidence/documentation-only publication, and those differences were reviewed.

Fresh raw proof:

* `/tmp/symaira-memory758-governed-independent-gate/receipt.json` and writes/deletes/fallback-boundaries plus three literal mutant failure files.
* `/tmp/symaira-memory758-governed-independent-baseline.json` and `.log`.
* `/tmp/symaira-memory758-governed-independent-read-controls.json` and `.log`.
* `/tmp/symaira-memory758-governed-independent-tests.log` and `...-cli-all-tests.log`.
* `/tmp/symaira-memory758-governed-independent-provenance.json`, `...-tree.json`, and this review's companion receipt.

Native CLI SHA256: `cb14ee41f457c5f2a93a153586ebd0b1e2bdb7b29a31378ddb1c50a9e2109204`. Frozen Go CLI SHA256: `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`. `go version -m` confirms go1.26.7, CoreKit v0.17.0, modernc.org/sqlite v1.59.0 and CGO_ENABLED=0. Actual executed helper builds use the pinned SDK and archived frozen Go tree; no frozen Go fixture was changed.

## Full-layer assessment and inspected scope

Read root AGENTS and migration ownership boundaries. Reviewed the entire governed delta against the normally integrated parent, every new/changed production body (CLI config/routing/write; Memory delete admission/entity/write/direct admission/embedding/lib/write), full new Set/Delete/boundary/gate Python runners and additive Go control/fallback wrappers, WRITE-CONTRACT, native workflow, public ADRs, migration ledger/matrix and provenance/retention manifests. Also inspected unchanged reachable flags/config schema/store/model/LSH/embedding plumbing and frozen Go command flags, SetGoverned/service/Prepare/Store, PII/extraction, entity resolution/save, memory scan/access/delete, governance and actual SDK float encoding. The earlier independent corrected Memory CLI review is carried as inherited evidence; normally integrated Windows borrow and SQLite handle closure changes are included in current source hashes.

Admission is conservative and fail-closed for regex compilation, ASCII content/metadata values, extraction triggers, PII candidates, raw non-UTF8 author/entities, invalid scope/typed duplicate metadata, conflict-enabled non-staged writes, unknown/malformed delete hydration, noncanonical dates/leap seconds and new-file modes. The frozen PII/extraction pattern sets and keyword list are represented conservatively; card/entropy candidates may delegate even where Go would retain them. Metadata keys, authors and entity names preserve their permitted Unicode/Go JSON escaping. Public defaults retain provenance/trust/policy and caller overrides; project discovery matches frozen Go boundaries. Successful model/source/dimension, finite float32 serialization, zero/sign-bit binary quantization, hashes/LSH, access count, audit identity/session and entity name-before-alias behavior pass full state replay. Existing malformed aliases are skipped with Go's best-effort boundary.

Delete's corrected observation order is independently exercised by all four added real callback cases: identity changes after access update use the post-update actor/session; removal during access update succeeds without a delete audit. Original source183 callback failure is retained and its literal Go `post-access &<>`/`post-session` versus old-native `original`/`seed-session` audit rows were inspected. The old executable remains hash-bound. The initially rejected FTS fixture, initial cascade assumption, two Clippy attempts and earlier wrongly targeted audit mutant remain distinct historical failures; none is claimed as an accepted final mutant or product parity result.

The service's autocommit save/audit/entity/kind/stage sequence deliberately matches Go; it is not advertised as an atomic transaction. DB repair's already accepted IMMEDIATE migration transaction and rollback safeguards are unchanged. No new dependency/lockfile, frozen Go, shared CoreKit source, Brain/Guard authority boundary or operator credential path was added. All ten newly changed Rust files relative to the integrated parent remain below 400 lines; the eleventh production file counted by author docs is the inherited normally integrated flags correction.

The CI workflow has pinned checkout/setup-go/upload actions, read-only token permissions, exact source receipts, retained logs on failure, locked Cargo checks, actual subprocess write/read gates and both families of real controls on Ubuntu/macOS/Windows. Windows `.exe` selection and closed SQLite handle ownership are present. Linux receipts are not native Windows/macOS acceptance. Native three-OS CI remains required after correction. Full #758, remaining PII/extraction/conflict/dedup/supersession/prefilter/JSONL/error/mode paths, MCP #760/#761, sync/serve #759/#762 and #649 release/Doctor ownership remain open; documents do not claim their completion.

The target and worktree are unchanged and released back to the parent/author after this review. Correct the two confirmed groups on a new source revision, retain these immutable failures, then obtain fresh exact-source process/controls/affected-test/native-CI evidence and independent verification. No merge approval is given for this candidate.
