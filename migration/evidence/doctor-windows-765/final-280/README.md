# Shared Go text and complete Setup JSON escaping: immutable local evidence

Source: `280914bc6bc9c3b3faac404dcd5d84856a424ed7`, clean throughout the
recorded executable gates. The later evidence-only publication does not change
this source. `validation.json` binds 50 actual test/CLI executable paths and all
reports to SHA-256. Brain CLI SHA-256:
`44468313653a364c28aedde3aa9536cbe0e27980b5cebe1d61f6e91aae1e764b`.
An exact CLI copy is `/tmp/symaira-doctor765-json-280-cli`.

The last independently reviewed source/publication (`a66b4818`/`34394496`)
remains unchanged. `../independent-review-a66/retention.json` preserves all 68
original proof/report/receipt files, including the eight actual JSON failures
and matching human controls. All 38 reviewed executables were losslessly
archived before target reuse; the receipt is
`/workspace/oracles/symaira-doctor765-a66-binaries/receipt.json`. Every archive's
compressed and round-trip executable hashes were checked again here. Earlier
46/35/24 retention maps and original Windows failure artifacts remain intact.

## Results

- 451 Rust tests pass, zero failed/ignored, 48 Cargo result summaries: 388
  CLI/Managed/Core tests and 63 additional normally integrated Guard tests.
- Locked all-target/all-feature Clippy for those five crates, workspace format
  checking, and all workflow actionlint checks pass.
- Actual frozen-Go/native Linux process comparisons: Source 111/111,
  Doctor 285/285, Setup 201/201; five intentionally corrupted process controls
  are rejected for their exact expected field.
- All 35 original boundary pairs and both actual SIGINT/SIGTERM owned-builder
  cleanup probes pass. All 23 original third-review consumer pairs match:
  eight install Setup, eight repair Setup, four Source, three Doctor. The eight
  JSON failures now close and the eight human controls remain matching.
- Standalone Guard regression: 124 cases, 121 full matches and three already
  approved explicit diagnostic states; zero audit diagnostic deviations.
  Guard raw paths: 63/63; all five real Guard controls are rejected.

Old named Source107/Doctor282/Setup169 cases are all retained. New permanent
cases cover complete JSON escaping alongside raw Unix owner bytes, literal
replacement characters, lexical owners, obstruction diagnostics and human
controls; Windows fixtures use legal platform filenames.

## Reproduction

Working directory: `/workspace/symaira-doctor765-json`. All compiler/replay
commands run beneath `/tmp/symaira-subreaper.py` with owned descendants:

```sh
python3 /tmp/symaira-subreaper.py bash -c '
set -euo pipefail
umask 022
. /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export CARGO_TARGET_DIR=/workspace/symaira-setup765-source/target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
cargo test --locked --all-targets --all-features -p symbrain-cli -p symbrain-managed -p symbrain-core -p symbrain-guard-core -p symguard-cli
cargo clippy --locked --all-targets --all-features -p symbrain-cli -p symbrain-managed -p symbrain-core -p symbrain-guard-core -p symguard-cli -- -D warnings
cargo fmt --all -- --check
/workspace/toolchains/bin/actionlint
scripts/setup-source-oracle/run.sh /tmp/independent-source.json
scripts/doctor-repair-oracle/run.sh /tmp/independent-doctor.json
scripts/setup-repair-oracle/run.sh /tmp/independent-setup.json
scripts/guard-standalone-oracle/run.sh /tmp/independent-guard.json
'
```

Each tracked oracle runner freshly builds its frozen Go executable from
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, records actual tool/binary/source
hashes, and retains complete process/state observations. The final raw replay
scripts and exact results are retained here under `symaira-doctor765-json-final-*`.
Run supplemental Python scripts under the same environment and subreaper.
The receipt builder is `symaira-doctor765-json-final-validation.py`; its original
run was after all executable gates and before evidence-only publication.
`logs.json` retains exact original log bytes as base64 with SHA-256.
The retained original `fix-consumers.py` ends with an intentional blank line;
its exact bytes are preserved rather than edited to silence whitespace checking.

## Platform and scope limits

Fresh extracted Windows SDK Join versus the actual managed-path assembly has
189,678 admitted nonempty HOME inputs matching. Its additional empty HOME
input differs and is retained; production rejects that input before assembly.
Generic Clean's 101 exploratory differences and both original assertion-failed
logs are also retained. These are host-Linux algorithm comparisons, not Windows
runtime/filesystem proof, and no comparison was weakened.

Narrow actual-source Windows GNU/macOS all-target Clippy passes with explicit
surrounding signature stubs. The initial missing `CARGO_BIN_EXE_symbrain` harness
environment error is retained; the successful retry uses an explicitly
unexecuted type-check placeholder. `signature-stub-harness.json` names every
included source and stub. Entire Setup CLI is tested on Linux; no native Windows
or macOS execution is inferred from the narrow harness.

Another full independent review and exact-head native CI remain required:
Windows Source95/Doctor269/Setup177, macOS Source111/Doctor285/Setup201. Historical
Go Browse/Swift worker builds remain transitional; broader typed configuration
diagnostics remain Go-owned. This checkpoint does not close #765/#783 or claim
a Go-independent source installation. Long-term rationale is recorded in
`docs/adr/2026-10-04-shared-go-json-setup.md`.
