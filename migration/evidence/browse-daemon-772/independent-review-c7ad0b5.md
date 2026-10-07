# Independent full-layer review: PR #801 / partial issue #772

Disposition: **changes requested** for two confirmed P2 path-contract defects. This read-only review does not approve merge or substitute Linux observations for all six native acceptance targets. Issue #772 remains incomplete: registry/autostart contracts and RUST-006 are explicitly pending.

## Immutable revisions and scope

- Repository: danieljustus/symaira-brain
- Reviewed HEAD: `c7ad0b5f98365d64139f0f139f936a1ecd08cdef`
- Main/base: `e8c7f7990ab61967db0a3cbadaf6e485b4a33d99`
- Tested source in existing receipts: `26d84df29033a882bd3bce557dbcf2cb1182bdb0`
- Supplemental frozen integrated Go: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`
- Historical oracle: `652453d1595fc302bd69c328e7da8a21dbee28b9`
- Worktree `/workspace/symaira-daemon772` was clean at review start and at source/hash/probe verification. Root began its correction in this shared worktree after the immutable C7 findings were delivered; the later dirty state belongs to that root implementation, not to this read-only reviewer. The findings and original probes remain bound to C7 and its verified binaries.

Read root AGENTS.md (identical applicable instructions previously read), the Browse contract matrix and work-item ledger, all 28 changed files listed below, and related Go/Rust registry, configuration, fetch-policy, runtime, protocol, frame-fixture and harness paths. No source/branch/PR/issue mutation was made.

## Confirmed findings

### [P2] Reject invalid cache environment values before choosing a profile root

`browse/crates/symbrowse-daemon/src/spec.rs:195` accepts every nonempty `XDG_CACHE_HOME`, including relative paths. Go `os.UserCacheDir()` explicitly rejects a relative XDG cache path; `browse/internal/daemon/session_registry.go:279` then uses `os.TempDir()/symbrowse/sessions`. The new Rust branch instead chooses `relative-cache/symbrowse/sessions` under the daemon's current working directory. This changes browser-profile placement and published session metadata, violating the intended OS-cache behavior and creating state in a project/worktree rather than the defined fallback.

Actual source/binary-bound process reproduction on Linux: private absolute HOME, config/data/state/runtime roots and TMPDIR, XDG_CACHE_HOME=`relative-cache`, empty PATH. Both immutable Go and reviewed Rust daemons completed start/session.info/stop with exit 0. Go reported `<owned TMPDIR>/symbrowse/sessions/<id>`; Rust reported `relative-cache/symbrowse/sessions/<id>`. Literal evidence: `/tmp/symaira-pr801-relative-cache-probes.json`.

The same resolver also accepts present-but-empty HOME on Linux/macOS and LOCALAPPDATA on Windows, producing relative paths instead of the missing-value fallback. The actual Linux Rust empty-HOME probe reports `.cache/symbrowse/sessions/<id>` and exits cleanly. Go CLI rejects that Linux environment earlier with configuration exit 9, so that separate probe is not claimed as a successful direct daemon-parity case. Go SDK 1.26.7 `os/file.go:504` confirms the underlying UserCacheDir empty-variable semantics. Literal evidence: `/tmp/symaira-pr801-empty-home-probes.json`.

Correction: reproduce UserCacheDir semantics for the applicable OS. Empty required environment values use the existing temp fallback. A nonempty relative Unix XDG_CACHE_HOME must use the temp fallback, not silently revert to HOME. Add isolated platform-appropriate path/process cases so these branches cannot hide behind the current three ordinary settings.

### [P2] Preserve per-byte Go replacement when exposing a raw worktree path

`browse/crates/symbrowse-daemon/src/spec.rs:207` obtains fallback origin through `cwd.display().to_string()`, and line 226 decodes git stdout with `String::from_utf8_lossy()`. Both collapse some invalid UTF-8 byte sequences to one replacement character. Go keeps the original path bytes until JSON encoding and replaces every invalid byte individually. The new origin metadata therefore changes the public path representation and can collapse distinct path representations differently from Go.

Actual Linux process reproduction used a privately owned POSIX CWD ending in raw bytes E2 82, empty PATH so both use CWD fallback, and absolute private HOME/XDG/TMP roots. Both daemons completed start/session.info/stop with exit 0. Go origin_path ended in two U+FFFD; Rust ended in one. Sources and executed binary hashes match the existing current candidate receipt. Literal evidence: `/tmp/symaira-pr801-raw-worktree-probes.json`.

Correction: retain raw OS/git-output bytes until an explicit Go-compatible byte-by-byte UTF-8 conversion, with platform-appropriate handling for native Windows paths. Exercise malformed multibyte CWD and git-output branches alongside ordinary Unicode paths. Do not normalize away origin differences in the comparator.

## Full-layer assessment

- Server split preserves the existing Unix/Windows transport ownership, shutdown/deadline, frame-limit and authorization logic. Read and compared parent versus extracted code, including same-UID Unix credentials, 0600 socket/0700 directory setup, startup lock and inode/device-checked cleanup; Windows owner ACL, remote-client refusal, first-instance contention, bounded workers, overlapped cancellation and joined handle teardown. No demonstrated new authorization or lifecycle weakening found. Existing partial migration contracts are not recast as complete.
- New configured policy reports static fetch SSRF as `!allow_private`, independently of browser SSRF. Actual runtime fetch uses allow_private, so the status repair describes its guard rather than weakening it. The private-literal error adapter only applies after an existing private-target refusal and parseable private literal; nonliteral DNS/rebinding/redirect diagnostics are retained. Redaction remains in the final error path. Unknown-command output matches the intended Go shape.
- Windows session paths now join separate components and preserve native separators. Initial real Windows mismatch and lint receipts are retained. Upgrade cache-directory extraction changes the Unix-only mutable builder binding and retains existing checks; CLI help-oracle style changes preserve assertions.
- Direct process runner demands the exact seven-command order across three policy settings. It checks Rust's documented mode/session extensions before removal, validates actual process PID and UTC timestamps within each process run, normalizes only the owned temporary root, and compares all remaining response/request data including network policy. Three validator mutations actually reject a missing case, wrong PID and changed fetch policy. No policy/authorization/config differences are erased by normalization.
- MCP companion checks the immutable source checkout, historical manifest, all four Go MCP source hashes and all thirteen existing raw input files. It runs real dev and v0.8.0 Go CLI processes, then real native CLI processes, comparing stdout bytes exactly with clean stderr/exit. Recovered owned-temp outputs feed existing raw-frame assertions through a real Rust production daemon. The fixture seam and session override have unchanged production defaults; they do not create a test-only proxy. Missing output, changed bytes and absent daemon controls demand actual cargo test failure with the intended diagnostic, rather than a zero-case pass.
- Workflow retains all six original runner labels, pinned checkout/Go, strict native lint, nonzero daemon tests, lifecycle, 50x50 races, actual Go/native process and MCP/workspace checks, and always-upload/error/14-day evidence. No Go production or frozen fixture changes occurred.
- Ledger validates 88 unique contracts and 17 acyclic work items. DMN-007/008 remain todo and RUST-006 in_progress. Existing 21/13 supplemental receipts are scoped evidence, not proof of complete registry/autostart migration.
- Crucially, the old Windows ARM failure is an immutable **Go** shutdown exit 1 with ERROR_OPERATION_ABORTED while observing SYMBROWSE_ALLOW_PRIVATE=true, before the affected Rust comparison. The preserved archive digest independently verifies. Earlier Rust native checks succeeding does not satisfy that entire native job, and the failed rerun request (workflow still running) is not a passed gate. Do not alter frozen Go or relax shutdown assertions. Fresh C7 native run 37141304343 must independently satisfy all six declared targets; this reviewer did not substitute any older head or approve a waiver.

## Independent verification actually performed

- Immutable HEAD and initially clean worktree verified; original diff whitespace check passed. Root correction began after source-bound original probes, so no clean-worktree assertion is made for the later shared state.
- Existing current source-bound receipt rehashed: 154/154 candidate source files match reviewed HEAD; 670/670 immutable Go source-tree files match the clean dcddcef0 worktree.
- All four historical MCP source hashes independently verified directly from immutable Git object bytes; historical manifest digest matches. Thirteen raw recorded Go/native byte pairs and all unchanged fixture inputs revalidated, including zero exit/empty stderr. Existing actual MCP failure-control records inspected, not rerun wholesale here.
- All 21 recorded process responses independently passed the strict comparator and its three mutation controls.
- Fresh actual current-native process run executed against immutable Go: 21/21 requests match; all three negative controls reject. `/tmp/symaira-pr801-independent-current-process.json` identifies exact C7 head and clean source.
- Existing built native daemon unit executable: 24 passed, 0 failed/ignored. Existing built native lifecycle executable: 6 passed, 0 failed/ignored. Executed under the owned subreaper with umask 022; no Cargo rebuild or source mutation. `/tmp/symaira-pr801-independent-checks.log`.
- Additional successful relative-cache and raw-CWD Go/native pairs and the separately scoped empty-HOME diagnostic reproduced the two findings above.
- Governance validator passed 88 contracts/17 work items. Existing full workspace 289-pass/64-summary and 50x50 race receipts inspected; not redundantly repeated. Native macOS/Windows/ARM hardware was not emulated.

## Changed files inspected

- `.github/workflows/browse-daemon-native.yml`
- `browse/crates/symbrowse-cli/src/upgrade.rs`
- `browse/crates/symbrowse-cli/src/upgrade/cache_dir.rs`
- `browse/crates/symbrowse-cli/tests/help_oracle.rs`
- `browse/crates/symbrowse-daemon/src/server.rs`
- `browse/crates/symbrowse-daemon/src/server/connection.rs`
- `browse/crates/symbrowse-daemon/src/server/helpers.rs`
- `browse/crates/symbrowse-daemon/src/server/tests.rs`
- `browse/crates/symbrowse-daemon/src/server/unix.rs`
- `browse/crates/symbrowse-daemon/src/server/windows.rs`
- `browse/crates/symbrowse-daemon/src/spec.rs`
- `browse/crates/symbrowse-daemon/tests/lifecycle.rs`
- `browse/crates/symbrowse-mcp/tests/raw_frames.rs`
- `browse/docs/rust-port/README.md`
- `browse/docs/rust-port/contract-matrix.json`
- `browse/docs/rust-port/implementation-plan.md`
- `browse/docs/rust-port/work-items.json`
- `browse/port/harness/daemon_mcp.py`
- `browse/port/harness/daemon_process.py`
- `browse/port/harness/run.py`
- `migration/evidence/browse-daemon-772/initial-diagnostic.json`
- `migration/evidence/browse-daemon-772/linux-mcp-process-report.json`
- `migration/evidence/browse-daemon-772/linux-process-report.json`
- `migration/evidence/browse-daemon-772/mcp-initial-mismatch.json`
- `migration/evidence/browse-daemon-772/windows-arm-go-shutdown-failure.json`
- `migration/evidence/browse-daemon-772/windows-initial-lint-failures.json`
- `migration/evidence/browse-daemon-772/windows-session-path-mismatches.json`
- `migration/evidence/browse-daemon-772/workspace-failure.json`

## Reviewer artifact SHA-256

- `/tmp/symaira-pr801-relative-cache-probes.json`: `548063b4e9aec58f676d40d29b3165605f854f5ba6cace534f93da30189180a8`
- `/tmp/symaira-pr801-empty-home-probes.json`: `979f41c71f6c4f6457fcf83c79f5df1b35d3b26c319793ca4e8a9e041fcdec13`
- `/tmp/symaira-pr801-raw-worktree-probes.json`: `eefc8050a14d5e3cf489715aa7663fe0e272b13dce94e30a41cca79bb22b6081`
- `/tmp/symaira-pr801-independent-current-process.json`: `88200aada0275f76de4832e5f0762bbdd715c3ff376f2dc180519d9287ba793b`
- `/tmp/symaira-pr801-independent-checks.log`: `5db2ae61e9c860a2fb2bbbc2704e7fe109bc20b41bb2732f81c51b8da762b047`
