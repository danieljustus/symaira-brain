# Clean bounded proxy-URI correction proof

Validated code source: `e2ecf4026806dd4d9fd219f15f931848b1fd56d9`.
Original rejected review: `52236234a197f00de89edbb185fc762a4f121161`
(code `1f82e9cf8812c5b383a19349241787215a857493`), preserved separately in
`../independent-review-522`. Frozen full Go oracle remains
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, built with Go1.26.7.
This directory is an evidence-only successor to the clean source commit.
`preservation.json` verifies every copied original byte, including verbatim logs.

Source38 introduced the bounded public Hyper raw-URI path, common proxy-auth
ordering, owned-body cancellation, exact process scope and E011 assertions.
Root's partial static review then identified the inherited Windows backslash
RUNNER_TEMP boundary. Source e2 changes only the runner's selected temporary-root
normalization before mktemp/tar and its ADR explanation. `source38-clean-history`
retains the earlier clean proof unchanged; it is history, not final-head proof.
The Windows finding was a static root review informed by another lane's actual
native CI failure; this lane has not executed native Windows proof locally.

## Actual clean-source results

- 251/251 original comparisons: 74 real HTTP and177 routing observations;
  all five original actual-process negative controls rejected.
- 32 exact Go/native proxy comparisons, seven Referer equality cases plus one
  specifically asserted E011 automatic-fragment sanitization, four Linux HTTPS
  proxy comparisons, four separately asserted native named-jar observations;
  three actual-output mutants rejected. Explicit caller Referer stays byte-equal.
- The exact original `/path` review reproduction closes all five raw-port pairs.
  The exact original eight HTTP pairs retain seven equal and only the authorized
  automatic Referer difference; raw request/response bytes remain in reports.
- 123 passed, zero failed, one parent-owned child test entry,26 summaries;
  seven actual isolated child executions are asserted by their passing parents.
  Fetch55 includes protected-resolver and live-client timeout/cancel/limit
  socket-closure assertions. Strict clippy, fmt and actionlint pass.
- 34 Fetch inputs,14 harness/workflow inputs,152 Rust release inputs and415
  frozen Go inputs are bound to immutable objects in `source-verification.json`.
  All locked package versions/sources remain unchanged; five existing transitive
  dependencies were explicitly exposed for supported HTTP/IO/TLS/body APIs.
- Current release builds bind Go SHA `221b047aa18e2c4eb421dd970de18e6e82372059c492f6bd52ac337c95c5a5f6`
  and Rust SHA `1900720a4c0b518f80747f7cfc8202b96b1778124072a6eafc138d6c67c4042f`.
  The complete coordinated quiet30×4 comparison passes unchanged gates:
  Fetch p95 -59.04%; binary-size reduction41.59%. All240 raw samples remain.
  The prior loaded failure is retained in the inherited1f82 evidence. A new
  metadata bootstrap failure before any sample (missing pinned SDK PATH in the
  outer wrapper) is also preserved, not presented as a successful measurement.
- Normalization-sensitive explicit-port measurement separately retains120 exact Go/native comparisons and
  every process-visible response interval. Ordinary HTTP/HTTPS proxy paths use
  one accepted connection for30 calls in each client. Raw-port native paths use
  30 connections, Go one. Raw HTTPS later-interval median was2.241ms native
  versus0.241ms Go; raw HTTP0.633ms versus0.174ms. These are local debug-seam
  response intervals, include startup in the first interval, and establish no
  global performance claim or alternate acceptance threshold. The selected
  path intentionally owns an unpooled connection per response.

Native six-target CI, Windows value and owned trusted-CA success on macOS/Windows
remain required. Complete#773 and#772 remain open. This is author evidence,
not independent approval, issue closure or release authorization.

## Reproduction environment

Use the existing owned target, with no duplicate build target. From `browse`,
run under `python3 /tmp/symaira-subreaper.py bash -c`, `set -euo pipefail`,
`umask022`, source `/home/agent/.cargo/env`, explicitly prepend
`/workspace/toolchains/go1.26.7/bin` to PATH, and set CARGO_TARGET_DIR to this
worktree's target, CARGO_INCREMENTAL=0 and DEV/TEST profile DEBUG=0.
Execute the tracked runner in the actual declared Browse CWD. `quiet-driver.py`
retains the unchanged standard driver/comparator invocation and load receipt;
`rare-cost-driver.py.txt` owns its HTTP/TLS fixtures and disposable HOME/XDG/CA
roots. CA trust is per process, never installed into an operator trust store.
The external original522 executable archive is SHA-bound in the retained receipt;
source38/final release Go binaries are separate actual build artifacts.
