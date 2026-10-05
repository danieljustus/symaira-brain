# Independent full-layer corrected Registry/autostart review #772

**Disposition: REQUEST CHANGES.** The original 48 invalid-session CLI failures are closed by the correction. One remaining P2 is independently reproduced for Unix raw session argument bytes, and one P3 affects newly exposed session help. No source/GitHub mutation or #772 closure is authorized by this review; six fresh native exact-head CI jobs and the separate state-key bridge remain required.

## Immutable artifacts

- Candidate/worktree: `/workspace/symaira-daemon772-registry`, clean `5ed6f1733f3bdaf86ef88eddb3616cece5431d8e`.
- Normally integrated main: `31de72294521701fc4b1a0bce39f03cc34d72e7e`.
- Original rejected candidate: `b363762cb85b879630fecd3fd141cba1ae85f71c`.
- Original registry implementation start: `a49b25285e8b241f8b0636c8d2b5da164bd63850`.
- Complete current Go source: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, clean `/workspace/oracles/daemon772-go-source`; pinned Go 1.26.7 Linux/amd64.
- Actual native executable `/workspace/symaira-daemon772-registry/target/debug/symbrowse`: SHA256 `b513cf31821d77af6248f70f76d8fcbbb0b53219672cb7106093cb7c21c6ac2b`.
- Actual reused source-bound Go dev executable `/tmp/symaira-pr801-go-1.26.7`: `99ba09243d5132c035485f96af2e2ddad24493ac8bced4f2a5bbb86c63daefe8`; version-v0.8.0 MCP executable `/tmp/symaira-pr801-go-mcp-v0.8.0`: `5f35bc2bc89753301e7151af4eb8dee45e7c80242719ec270827708eb2a183dc`.

The reviewer independently read full production/test/harness/CI/docs layers, inspected the correction separately against b363, reran actual Registry/baseline/MCP/workspace gates and supplied new literal CLI inputs. Author approval or author-only test results do not establish this disposition. Candidate-owned existing target was exclusively reused after release coordination. No build duplication, real browser/credential/operator state, signing/release or extra agent was used. Real processes ran beneath `/tmp/symaira-subreaper.py` with umask 022 and pinned SDK PATH; incremental/dev/test debug were disabled. The actual Go API supplemental test executed through a read-only overlay and left the frozen checkout clean. Go CLIs were verified existing binaries, not claimed to have been rebuilt by this reviewer.

## P2: preserve raw Unix session bytes before rendering the corrected CLI diagnostic

Locations: `browse/crates/symbrowse-cli/src/main.rs:1283-1286`, feeding `browse/crates/symbrowse-cli/src/daemon_state.rs:84-90`.

The correction correctly selects the generic CLI internal envelope, full grammar message and explicit exit 1 before endpoint/client construction. However, `parse` first converts every OsString through `to_string_lossy`; `validate_cli_session` therefore receives replacement Unicode characters rather than the literal Unix argument. The Go-compatible scalar quote helper cannot recover discarded bytes. This is the remaining byte-identity case in the same CLI validation contract, not a raw daemon protocol issue or an authorization bypass.

A fresh actual comparison of 120 CLI pairs has **84 matches and 36 mismatches**. Three raw rejected names (`bad\xffsession`, `bad\xe2\x82session`, `bad\xc0\xafsession`) differ for each of session list, session info, daemon status and state list, in text/JSON/YAML. Both processes now exit 1, and the machine error code remains internal; the diagnostic bytes differ. For `bad\xffsession`, actual Go text is:

```
invalid session "bad\xffsession": use 1-64 letters, digits, '.', '_' or '-'
```

Rust instead writes a literal replacement character in `"bad�session"`. Truncated/overlong byte sequences also coalesce or replace differently. Actual valid UTF-8 U+FFFD is a distinct control and matches Go; so replacing all replacement characters with byte escapes would corrupt valid input. Empty, path-traversal, invalid first character, overlong ASCII, newline, quote/backslash and tab controls also match. With autostart disabled and private HOME/XDG/runtime/cache/state/temp roots, before/after inventories prove that none of the 120 invalid cases creates daemon, profile or state files.

Exact literal args, exit, stdout/stderr bytes, candidate/binary identity and results: `/tmp/symaira-registry-772-corrected-independent-extra-cli.json`. Reproducer `/tmp/symaira-registry-772-corrected-independent-extra-cli.py`; adjacent `.log` gives the aggregate and failing groups. All observation data uses hex/base64, preserving the original bytes.

Correction: retain the final selected raw session value through CLI parsing until validation and Go-compatible byte quoting. Preserve selection/last-override precedence and help/format behavior; valid ASCII session operation can still use the existing string/domain interfaces. Add actual Unix-only raw-byte cases with valid U+FFFD controls, ideally both inline/separated values and selected global flag positions. Preserve the current 36 failures and original b363 evidence. Keep raw protocol `invalid_session` and its separate message unchanged. Fresh six-native-target CI remains required; Unix raw bytes are not a Windows argument claim.

## P3: retain the Go wording in newly exposed session help

Location: `browse/crates/symbrowse-cli/src/daemon_state.rs:165-168`. The new help constructor invents “Inspect daemon-owned sessions” and “List daemon-owned sessions”, whereas actual frozen Go describes the same supported paths as “Inspect browser sessions” and “List sessions”. These changed bytes escape the existing help oracle, whose supported paths derive from root discovery and do not exercise this newly exposed fallback. Session info help already matches exactly.

Six actual clean CLI pairs (`session --help`, bare `session`, `session list --help`, `session info --help`, `help session list`, `help session info`) prove four mismatches and two exact controls, with exit 0 and no stderr/writes in both implementations. Exact bytes and executable/source identities: `/tmp/symaira-registry-772-corrected-independent-help.json`; source-bound reproducer and log are adjacent `.py` and `.log` files.

Reuse the pinned Go help wording and add explicit actual help comparisons for the new supported session paths. Unsupported session-id remains outside this increment, so root help can retain the existing supported-command filter; this finding does not require advertising an unimplemented command. Preserve the intentional partial CLI scope while retaining the descriptions and format contract of the implemented commands.

## Original finding closure

The correction affects exactly the shared CLI adapter plus ServerError socket-validation display, process harness additions, matrix/docs and preserved original evidence. It does not globally change Core error exits, raw protocol frames or state-key behavior. Both lifecycle and state adapters invoke validation before endpoint/client construction. The display reuses the already exhaustive Go scalar quote formatter and complete existing socket grammar. The renderer explicitly supplies internal/exit1; generic error rendering elsewhere retains prior classifications.

Fresh Registry comparison contains **108 CLI observations per implementation**, including all **48 original invalid spellings × verbs × formats**, **20 recorded raw frames**, **eight concurrent autostart clients**, **seven actual Go constructor-root observations** and **three freshly Go-produced persisted files**. All original 48 argument/exit/stdout/stderr records compare literally, without path/PID/time projection. Six mutations of actual receipt observations are rejected: foreign/previous owner, absent diagnostic, invalid timestamp, changed CLI exit and changed protocol error code. These six are comparator mutations, not independently compiled mutant executables.

Original b363 review/report/48-failure pairs in `migration/evidence/browse-registry-772/independent-review-b363.md`, `.json` and `independent-invalid-cli-b363.json` are independently byte-identical to the original `/tmp` documents. The rejected original executable remains verified at `/tmp/symaira-registry-b363-rust`, SHA256 `91de2c212944e742e509bcc805136e90e98e2bda14c917e2bd553823a868a808`. No historical failure or review was rewritten.

## Complete layers inspected

Production: focused client connection/deadline, I/O/error/provenance, config defaults and launch/kill/reap modules; server dispatch/shutdown and Unix/Windows listener ownership; registry root cleanup/defaults, validation, ensure/touch/list/info/restart/ref copies; main/session/state CLI parsing/output/error adapters; state model null compatibility and runtime inspection/cleanup; shared YAML rendering; pinned Go scalar quoting/Unicode range table; cache helper extraction. Accepted main Activity changes are not reattributed to Registry. All changed-input hashes and correction delta paths are retained in the accompanying receipt.

The native client retains separate per-request connect/read/write bounds and startup retries, with bounded best-effort owned-child cleanup. The startup timer is not an absolute cap on an already connected read; actual frozen Go also separates these budgets. Ordinary clients retain one-request dispatch; explicitly configured engine/policy clients verify owner configuration before mutation, including no-autostart requests. Registry state and ref maps remain private; returned maps are copies, restart clears memory and retains profile bytes. Actual owner IDs/times and private mode checks are validated before projection.

Unix listener keeps current-user peer checks, lifetime startup lock, private endpoint permissions and inode/device-owned stale-socket removal. Windows statically retains owner-only local named-pipe ACL, first-instance ownership, bounded overlapped cancellable reads/writes, shutdown/wake and joined native handle cleanup. This Linux review does not turn that static inspection or cross-compilation into Windows runtime proof. Dispatch and shutdown share their transition gate, and state inspection retains current schema/provenance. Existing browser/vault policy and MCP exposure are not expanded by the CLI diagnostic repair.

Tests/harness: client/registry/config/root isolated parents and ignored child entries, cancellation test changes, main/help tests, refreshed raw MCP fixtures, all Registry/Go-overlay process comparator inputs and assertions, projections, CLI literal comparisons and every rejection control, ordinary lifecycle harness isolation, six-runner workflow, public ADR and scope ledgers. Registry normalization first checks the correct owner for each lifecycle phase, UTC timestamp interval, owned prefix/path suffix and stable error/policy fields; arbitrary diagnostics are not silently normalized. Production Go/frozen fixtures have no changed paths. New extracted production files are focused and bounded; preexisting large CLI/runtime modules are reduced rather than credited as newly fully split modules.

## Independent execution and provenance

- Registry: 108 CLI/20 frames/eight concurrent clients, seven actual Go API root cases, three persisted fixtures, six receipt mutation controls; exact source-bound receipt `/tmp/symaira-registry-772-corrected-independent-process.json` and its `.raw.json`/`.log`.
- Integrated parent: 63 actual paired daemon requests and three receipt mutation controls pass in `/tmp/symaira-registry-772-corrected-independent-base-process.json` and `.log`; raw invalid-session behavior remains correct.
- MCP: all 13 actual CLI pairs against both Go version variants pass in `/tmp/symaira-registry-772-corrected-independent-mcp.json`. All four historical MCP source hashes still match. Three actual Cargo fixture/daemon failure controls exit 101 and require their intended assertion plus exactly one failed test, rather than accepting setup failures.
- The fresh MCP-owned production daemon/Go fixture supplies the complete locked workspace replay: **301 passed, zero failed, three deliberately ignored owned child entry points**, 66 summaries. Those child entries execute through their isolated parents. Literal log `/tmp/symaira-registry-772-corrected-independent-mcp.json.tests.log` contains no compilation lines.
- Independent strict locked workspace/all-target/all-feature Clippy, workspace format check, native workflow actionlint and contract validator (88 contracts, 17 work items, valid links and acyclic DAG) pass. Source remains clean.

All 170 candidate files and 670 complete frozen Browse files in both fresh process manifests independently match current bytes and exact immutable Git objects. All 14 author handoff receipt/log hashes and four retained executable hashes match. Fresh Go supplemental API observes all 1,112,064 valid Unicode scalar quotes at the pinned digest `4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055`; fresh native workspace executes that same exhaustive property. The static 712-range/Unicode15.0.0 provenance is checked and remains unchanged. This proves valid-scalar quoting; the raw argument finding concerns prior byte loss.

The three fresh Go files were independently parsed as framed `SYMBROWSE-STATE\0` + header + payload, with schema3, key_source none, null cookies and retained synthetic local storage. Metadata/cleanup gates assert retained alpha file byte equality and absence of state-value disclosure. Detailed framed-file digests: `/tmp/symaira-registry-772-corrected-independent-state-fixtures.json`. These plaintext fixtures do not prove the unresolved encrypted-state bridge.

Author additional exact-head logs independently hash-verified: two exit harness tests, actual lifecycle/stale socket and 50 rounds × 50 competing starters pass. They are reported as author evidence, not a duplicated independent race run. The actual source/frozen checkout remain unchanged after read-only overlays.

## Open acceptance and target release

DMN007/008 remain fixture-ready; RUST006 and #772 remain open/in progress. Both runtime Store call paths still pass None; the key bridge is explicitly a separate blocked baseline/worktree. No protected state downgrade/key migration is approved here. Six fresh native platform/architecture jobs (Linux/macOS/Windows × x86_64/arm64) must pass at the final corrected source. Original Darwin/Windows Go blockers and cutover limits remain visible.

The current raw-byte and supported-help findings require a focused correction, preservation of all failure observations, and independent closure. No source edits/push/PR/merge were performed. Final owned-target executable/CWD/FD scan finds zero users; no compiler or replay remains active. Hash-bound receipt: `/tmp/symaira-registry-772-corrected-independent-review.json`.
