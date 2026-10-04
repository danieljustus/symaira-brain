# Resolve one validated Memory HTTP request authority

Status: accepted implementation decision; Linux/native-three-OS validation and
different-author review pending. Refs #763, #759 and #762.

Independent review of source824/publication3f4 found one functional P2 in the
new native owner. Multiple Host headers selected only their first value, and an
absolute request-target's authority was ignored. Actual supported GET and
authenticated POST requests passed the native guard while frozen Go rejected
them before any database change. Native POST committed real memory/audit/sync
state; the original requests, responses and all-SQLite snapshots are preserved
unchanged under `migration/evidence/memory-ui-763/independent-review-824`.

Resolve the effective request authority once before rate limiting, JWT, body
collection or Store operations. Keep Go1.26.7's Host cardinality and missing/
malformed-header requirements. An absolute request-target supplies the effective
Host, including Go's removal of URI userinfo; otherwise use the sole Host header.
An HTTP/1.1 absolute URI does not excuse a missing Host header. CONNECT retains
Go's existing missing-header exception without adding any tunnel route.

Pass that same authority to existing loopback validation and exact-listener
same-origin admission. Keep JWT, CSRF, profile, revocation and conservative
write/read checks. This fixes a supported owner boundary; deferring the full
serve cutover does not justify accepting the incorrect request routing. It also
avoids a second URL parser, new dependencies or a general transport rewrite.

Go's early Host errors are plain 400 responses without application security/CORS
headers, and close the connection before reading the body. Preserve their body
and reason diagnostics, including the missing-Host distinction. Full HTTP wire
serialization and unrelated parser/URL/timeout behavior remain separately gated.

The correction is authored in an isolated successor after normal integration of
main2b6d49f. The documentation conflict preserves both accepted Console/TUI
decisions and distinguishes main's12 Go/native probes from the original13
Go-only invocations. Original source824 and every retained failure stay unchanged;
the nonfunctional replay.py whitespace cleanup is confined to the successor.

Acceptance requires the original12 finding pairs and eight transport controls,
specific positive local/foreign/order/Origin/no-auth requests and committed-state
bindings, full52 HTTP comparisons/seven all-SQLite refusal boundaries/two real
failure controls,18 desired same-origin pairs, actual DOM11/original5 and the
inherited590 read/60 Set/16 Delete/13 delegated/10 failure-output contracts with
their actual controls. New negative executions must make the authority gate
fail. Actual native three-OS CI and a different author must review the successor
before publication. No complete #763 closure, native `memory serve` admission,
historical Store migration, graphical browser or native Windows/macOS claim is
made by this implementation checkpoint.
