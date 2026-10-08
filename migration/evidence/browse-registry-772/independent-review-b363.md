# Independent full review: daemon Registry/autostart #772

Clean immutable candidate b363762cb85b879630fecd3fd141cba1ae85f71c in
/workspace/symaira-daemon772-registry. Reviewed complete increment against
original published a49b25285e8b241f8b0636c8d2b5da164bd63850, excluding unrelated
Activity implementation already accepted on normally merged main31de722.
Verdict: REQUEST CHANGES. One confirmed P2 finding; no source edits.

## P2: preserve invalid-session CLI validation and exit classification

browse/crates/symbrowse-cli/src/daemon_state.rs:14 validates the lifecycle CLI
through an Io/internal error, Rust Debug quoting and generic error.code.exit_code.
The new session list/info and daemon status paths exit7, whereas actual Go exits1,
uses Go quoting and includes the diagnostic ': use 1-64 letters, digits, ...'.
State list instead exits2 with invalid_session, also different from Go's exit1
and internal CLI envelope. Even the ordinary 'bad session' input reproduces it;
this is not just a Unicode parser corner case. The daemon's raw protocol invalid-
session contract is separate from the CLI adapter's generic validation contract.

Forty-eight fresh actual Go/native CLI pairs (session list/info, daemon status,
state list; ordinary space/ESC/combining mark/soft hyphen; text/JSON/YAML) all
differ with autostart disabled, absent sockets and private HOME/XDG. For example
session list --session 'bad session' returns Go exit1 plus the complete validation
message; Rust exit7 plus only 'invalid session "bad session"'. JSON preserves
internal but its message and exit are wrong. State list returns a different
protocol code as well. Invalid input must retain Go's CLI envelope/message/exit
without creating profiles or spawning a daemon. Apply shared Go quoting through
all affected CLI adapters and validate before transport construction; keep the
correct raw-frame invalid_session response separate. Add real CLI failure cases
to the permanent process gate. Literal requests/exits/stdout/stderr and source/
binary binding: /tmp/symaira-registry-772-independent-invalid-cli.json.

## Full reviewed layers

Inspect every production change: focused client connect/transport/errors/launch/
cleanup modules, constructor/config defaults, one-request ordinary CLI and explicit
engine/policy validation before mutation, registry lexical/default roots/private
profiles and copied ref tables, ensure/touch/list/info/restart behavior, shared
server dispatch/shutdown gate, raw frame encoding and scalar quoting/range table,
state model null compatibility and inspection schema/cleanup, CLI error/format
adapters and common YAML changes. Review all changed Rust tests and cancellation
adjustments, API overlay and actual-process harness, allowed projections and each
failure control, fresh/frozen Go manifests and raw outputs, six-target workflow,
ADR/evidence preservation and matrix/work-item/implementation scope. New focused
modules are below400 lines; preexisting large CLI/runtime files are reduced by
extracting behavior. Normal main ancestry verified. Historical Go/frozen fixtures
remain unchanged. No other actionable finding in the reviewed increment.

The client retains per-connected-request read_timeout as well as the separate
startup retry timer and bounded owned-child kill/reap cleanup. The startup timer
is not an absolute cap on a socket request that is already connected; frozen Go
also has separate request and retry deadlines. Explicit engine/policy checks occur
before dispatch for both ordinary and no-autostart requests; existing MCP stronger
status verification remains intact. Local tests exercise private permissions,
owned child reaping and rejected incompatible-owner mutation. Registry roots use
actual platform cache/temp rules and lexical cleanup; restart discards memory/
refs while retaining profile bytes. Session names remain strictly validated.

## Independently executed evidence

Fresh source-bound Registry gate passes60 actual CLI observations and20 raw frames
per implementation, eight concurrent autostart clients each, seven actual Go API
constructor-root cases and three freshly Go-produced persisted files. Typed
normalization validates owners per lifecycle phase, timestamps within the actual
run and exact owned path prefixes before replacing nondeterministic values.
Error details, policy, session names and suffixes remain compared. Four mutations
of actual process receipts are rejected. These are comparator mutation controls,
not four independently compiled executable mutants.

Fresh integrated baseline passes63 actual request pairs; three receipt mutation
controls reject missing case/foreign PID/changed network policy. Fresh MCP passes
all13 actual CLI byte pairs against dev and v0.8.0 Go executables; all four original
Go MCP source hashes still match the historical manifest. Its three actual Cargo
failure controls return101 for missing/changed fixture and missing daemon, require
specific assertion diagnostics and one failed test, and do not count infrastructure
failure as rejection. Also run its full workspace gate through a fresh live native
daemon and newly Go-produced fixture:301 tests pass, zero fail, three deliberately
ignored owned child entry points remain visible and are executed by isolated
parents. Existing target reused; the fresh workspace log contains no Compiling
lines. Record exact executed test-binary digests. Author's same301/0/3 record is
separately verified; race50x50, exit harness2 and strict lint logs are checked as
author evidence, not falsely described as a second independent execution.

Independently verify170 candidate files and670 complete frozen Browse checkout
files against fresh reports/current bytes and immutable Git objects. All14 handoff
receipt/log digests and all three literal executable digests match. The actual
Go source remains clean DC before/after readonly overlays. Fresh actual Go scalar
quote output equals SHA4c752b4c6e90df8c641d6ac02a6da80113943bc8fc476c825f27aef51db2a055
for all1,112,064 valid scalars. Independently rerun the range generator, verify
Unicode15.0.0 and712 ranges byte-for-byte against the checked-in table, and verify
both generating probes' provenance hashes. Fresh Rust workspace executes the
exhaustive scalar-quote property successfully. This proves the formatter; the CLI
finding shows missing integration of that formatter and validation contract.

Fresh actual persisted fixtures have the SYMBROWSE-STATE magic, v3 header and v3
payload, cookies:null, and retained local storage. Inspection/clear/clean do not
expose stored values or modify retained alpha file bytes. Initial reviewer audit
mistakenly parsed the whole framed file as plain JSON; after reading the codec,
corrected audit explicitly validates magic/header/payload. No production failure
or data rewrite is inferred from that tooling error.

## Remaining scope and disposition

State-key bridge remains a separate explicit blocker: both relevant runtime Store
construction paths still pass None. This review neither waives encrypted migration
data preservation nor claims complete #772/RUST006. DMN007/008 fixture-ready status,
six fresh exact-head native Linux/macOS/Windows architecture gates, original Go
WindowsARM shutdown/Darwin socket failures and full cutover limitations remain
visible. No platform cross-compilation is substituted for native process evidence.

Original reviewed source, target, Go checkout and frozen fixtures remain unchanged.
No GitHub writes, PR, publication or additional target builds. Concrete review
receipt: /tmp/symaira-registry-772-independent-review.json; all fresh literal
receipts and test logs use /tmp/symaira-registry-772-independent-* paths. Candidate
can be corrected in a separate worktree while retaining these source-bound failures.
