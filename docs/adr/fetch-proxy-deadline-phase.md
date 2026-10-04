# Proxy deadline tests admit a phase before controlling time

PR #810's two native Linux failures reached Timeout, then observed peer EOF
before a complete request header. The old fixture unwrapped that valid early
EOF and dropped its completion channel. The failure did not demonstrate a
leaked connection, and cannot be waived. Full original logs, ZIP, triage,
30ms input and the exact archived test ELF are retained with hashes.

The successor changes tests and the existing pinned Tokio dev feature only.
Production deadline, socket/task/body ownership, proxy authentication, policy,
request selection and retry behavior remain unchanged. Real sockets still
provide the close observation while the client stays alive.

The public test requires a complete, exact raw080 request at an owned peer that
withholds response headers. Only after that phase does it control Tokio time.
It proves pending below the armed budget, actual Timeout when that budget is
exceeded, then resumes real time and requires bounded OS EOF/reset and task
join. The shipped30s setup budget cannot become a slow wall-clock test: the
operation deadline advances under test control after admission.

A separate private test obtains the actual Response through production
redirect::send, including raw proxy_uri::send and OwnedBody. After partial-body
admission it arms the original30ms timeout over the production read_response,
polls the reader to Pending and advances31ms. Awaiting the timeout by value
drops its inner response at the same boundary as honest::fetch. Real EOF/reset
and peer-task join must follow before client teardown. Keeping these phases
separate avoids mixing virtual time with production std::Instant when a new
body timer is calculated. Production clocks are not changed for testability.

The fixture reports early EOF with exact header bytes instead of losing its
sender. Late-phase tests require that late phase explicitly. Bound header bytes,
observed task results and abort-on-drop cleanup retain every completed action;
no error becomes a passing skip. Complete-response controls exercise successful
Fetch and actual body decoding rather than attributing all closes to failure.

Three actual test-ELF controls deliberately retain the timed body owner, retain
the real ConnectionTask socket guard, or advance insufficient time. The same
positive closure/expiry assertions must reject each precise defect, with full
raw output preserved. They do not invent a Fetch response or alter production
Drop behavior. Dev-only test-util uses the unchanged pinned Tokio1.53.1;
there is no new production dependency or lockfile upgrade.

Local Linux results need full different-author review and exact-head native
six-platform CI. Existing pooling cost, origin-byte-auth gaps and other open
Fetch/Daemon acceptance criteria remain visible. This correction does not close
#773 or authorize an unproved production cutover.

## Independent full review

The bounded correction is approved after a different-author full review and fresh independent303 workspace tests,13 actual MCP pairs,251 Fetch comparisons,32 raw proxy pairs,114 credential pairs and all original negative controls.32 additional actual test-ELF repetitions exercise128 body/header/earlyEOF/close/task cases; the three precise actual mutation controls reject. Strict workspace checks and the88-contract matrix pass. All22 production-file comparisons,69 retained author artifacts, source maps and original152/121 archived identities verify. Complete independent proof is under migration/evidence/fetch-deadline-773/independent-6eb3cde.

Main2b6d49 is integrated normally and adds only documentation/history; reviewed production, tests, harness, workflow and Cargo remain byte-identical. Current protected and genuine six-platform native checks still must pass. This addresses the flaky fixture without changing production timers, does not waive the original Linux jobs, and does not claim a fresh benchmark or full773 cutover. The prerequisite final801 integration remains required.

Workspace builds can change a shared example's feature-graph binary without changing source. Preserve each actually executed binary SHA/archive before target reuse, and separately record the current variant; do not require the old execution identity to remain at a mutable cache path. All149 current cache ELF paths were verified against immutable archives before the exclusively owned1.7GB cache was retired. Original failed provenance assumptions and all receipts remain retained.
