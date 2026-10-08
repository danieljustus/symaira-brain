# Memory HTTP authority correction: authored Linux evidence

Tested source: `b7fb40fda3ec68526ad5b4eb5e05b285d3f00935`; original publication3f4
and source824 remain unchanged. Main `2b6d49f250a81650eda1b93cec7d859a171873bb`
is normally integrated. This is author validation, not independent approval.

The supported owner rejects duplicate Host headers before rate limiting,
authentication, body collection or Store access. It preserves Go's missing and
malformed Host diagnostics, and uses the absolute request-target's raw authority
for the existing loopback/actual-listener same-origin checks. URI userinfo is
removed without canonicalizing host/port spellings. JWT, CSRF, profiles,
revocation and write admission are unchanged. The public rationale is
[the authority ADR](../../../../docs/adr/memory-http-request-authority-763.md).

All clean-source gates passed on Linux:

- 325 native tests, 33 summaries, zero failed/ignored; strict combined Memory/CLI
  all-targets/all-features Clippy, workspace fmt and all-workflow actionlint.
- 52 actual Go/native HTTP pairs, seven native 501 complete-SQLite no-write
  boundaries and two actual full-replay failure controls.
- 15 authority pairs, including all 12 original finding inputs, with committed
  write UUID/timestamp/actor/audit/FTS bindings. Two actual request mutants fail:
  omitting the native duplicate Host or replacing its foreign URI authority.
- All eight original raw transport controls, 18 desired same-origin pairs,
  actual jsdom 11 native checks and five original-defect checks.
- 590 CLI read cases and two actual subprocess controls, each rejecting all32
  seeded read comparisons; 60 Set,16 Delete,13 delegated boundaries,10
  failure/output comparisons and three actual identity/audit/exit controls.

The original12 finding and eight transport requests are byte-identical after
substituting only their owned ephemeral loopback listener-port bytes. Their
original outcomes, raw requests/responses and complete SQLite snapshots remain
unchanged. Early 400 reason/body diagnostics now match Go; full Date/chunking/
header-order wire serialization is not asserted.

[validation.json](validation.json) binds 546 candidate source/dependency/
workflow inputs, all2,438 immutable Go files, all33 actually executed test
binaries and four primary native/Go executables. The 37 distinct actual binary
byte sequences are gzip/SHA round-trip preserved under
`/workspace/oracles/symaira-memory763-authority-b7fb40f-binaries`. This selected
archive does not relabel stale auxiliary target artifacts as current proof.
All148 new raw proof files are retained with original/gzip hashes. The verifier
also rechecks all232 original independent files (231 proofs plus final receipt),
459 original author files and the original526 ELF paths/492 unique archives.
Old target paths were reused only after their verified pre-build archive; current
executables are never attributed to source824.

The initial source6c6 test failure is preserved. Its new test assumed a
percent-encoded ASCII hostname passed the locked http1.5 URI parser. The
production code was unchanged; sourceb7fb asserts existing parser rejection.
The completed finalizer stdout replaced its initially captured empty in-flight
snapshot in metadata only; the complete original enumeration and original
finalizer are preserved. No gate result/process/state was rewritten or rerun
for that metadata correction.

The sole released target and default11434 were exclusive during their gates.
After all subprocesses finished, `/proc` showed zero target users and11434 was
empty; compiler and endpoint ownership were released. No operator HOME,
credentials or paid embedding endpoint was used. Go control/fallback wrappers
were compiled from retained inputs, hashed in the unchanged runners' receipts
and disposed with their owned temporary roots; their executable bytes are not
claimed retained.

Use only the coordinator-released existing target; no second target:

```sh
set -euo pipefail
umask 022
. /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export CARGO_TARGET_DIR=/workspace/symaira-memory763-ui/target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_BUILD_JOBS=2
cargo build -p symbrain-memory --example http_probe --offline --locked
cargo test -p symbrain-memory -p symbrain-cli --all-targets --all-features --offline --locked
cargo build -p symbrain-cli --bin symbrain --offline --locked
cargo clippy -p symbrain-memory -p symbrain-cli --all-targets --all-features --offline --locked -- -D warnings
cargo fmt --all -- --check
/workspace/toolchains/bin/actionlint
python3 /tmp/symaira-subreaper.py python3 scripts/memory-http-oracle/run.py \
  --native "$CARGO_TARGET_DIR/debug/examples/http_probe" \
  --node /opt/codex/runtimes/codex-primary-runtime/dependencies/node/bin/node \
  --jsdom /tmp/symaira-memory763-dom/node_modules/jsdom --report-dir /tmp/new-authority-http-proof
python3 /tmp/symaira-subreaper.py python3 scripts/memory-cli-oracle/replay.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 --rust "$CARGO_TARGET_DIR/debug/symbrain" \
  --report /tmp/new-authority-read-proof.json
python3 /tmp/symaira-subreaper.py python3 scripts/memory-cli-oracle/controls.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 --rust "$CARGO_TARGET_DIR/debug/symbrain" \
  --report /tmp/new-authority-read-controls.json
# Coordinate exclusive11434 before write_gate.
python3 /tmp/symaira-subreaper.py python3 scripts/memory-cli-oracle/write_gate.py \
  --go /workspace/oracles/symbrain-go-dcddcef0 --rust "$CARGO_TARGET_DIR/debug/symbrain" \
  --report-dir /tmp/new-authority-write-proof
python3 migration/evidence/memory-ui-763/final-authority-b7fb40f/verify.py
```

The evidence-only successor does not change frozen production/harness/workflow
bytes. Genuine Windows/macOS/native-three-OS and protected CI plus a different
 author's full review remain mandatory. No native CLI `memory serve` cutover,
full #763 closure, complete HTTP lifecycle/parser equivalence, graphical browser
proof, historical Store compatibility or full typed configuration/key/policy/
retrieval/sync/relay/stats admission is claimed.
