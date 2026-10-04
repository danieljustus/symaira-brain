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
