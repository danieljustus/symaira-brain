# Independent full Memory HTTP/Web owner review (#763)

Disposition: **REQUEST CHANGES** for one confirmed functional P2. The bounded
owner, shared-store changes, assets, harness, workflow and retained evidence
were reviewed together. No source or GitHub change was made by this reviewer.

Candidate: `/workspace/symaira-memory763-ui`, clean publication
`3f4c28701c5454f0b2b42090db0706fce8d0857c`; validation source
`824a95e0bdbe6a25108e1dbadaaebf7d89dc8cc4`; production source
`d1f4faa77dc722da59a9cea1ff000566ce033f8f`. Normal approved Store parent
`27001b1bd6948cd7985773655f3a43bc41e947b3` and main `e3dbda6` are ancestors.
The evidence-only successors change no Rust/assets/Cargo/workflow bytes.

## P2: Resolve and validate the complete request authority before admitting routes

`rust/symbrain-memory/src/http/routes.rs:36` and `:59` independently read the first `Host` header for
Host, CSRF/CORS and exact-listener origin admission. It never rejects additional
Host occurrences and does not use an absolute request-target's authority.
`middleware::header` uses `HeaderMap::get`, so a loopback first header conceals a
later foreign header. This affects actual supported GET and direct-write routes;
it is not only a hypothetical complete-server cutover concern.

The retained twelve real frozen-Go/native pairs in
`/tmp/symaira-memory763-independent-host.{py,json,log}` bind literal request and
response bytes, the actual executable hashes, every SQLite table/column/row/blob
before and after, returned actor/provenance and actual FTS integrity checks:

* Authenticated POST `/api/set` with `Host: 127.0.0.1:<listener>`, a second
  `Host: untrusted.invalid`, and the real local Origin: Go returns 400 with all
  SQLite state unchanged; native returns 200 and commits an additional memory,
  audit and sync state. The actor is the authenticated subject and HTTP kind is
  empty, so this is a real admitted write, not an empty response comparison.
* The same duplicated headers on `/api/list` produce Go 400/native 200. Reversing
  header order produces Go 400/native 403. Two identical local Host headers also
  produce Go 400/native 200, proving cardinality must be validated independently
  of the first value's loopback status.
* Absolute-form `http://untrusted.invalid/api/set` with a local Host and exact
  local Origin produces Go 403/no write versus native 200/a real second commit.
  The corresponding GET is Go 403/native 200. The reverse GET, local absolute
  authority with a foreign Host, is Go 200/native 403. Go explicitly selects the
  URL authority before the header; the native owner ignores it.
* A missing HTTP/1.1 Host gives Go 400/native 403. A duplicated-Host POST without
  a JWT still gives native 401 without any database change. A single local Host
  with valid JWT remains a positive real write in both owners. These controls
  distinguish the authority mismatch from an authentication bypass or broken
  fixture.

This is one functional P2 covering Host cardinality and effective authority.
Fix the shared request admission boundary narrowly: preserve Go's syntactic
Host requirements, use the effective authority consistently in Host/CSRF/CORS
checks, and reject invalid cardinality before any operation. Preserve the
approved intentional exact-listener same-origin behavior and all original raw
observations. No broader HTTP rewrite or new scope is needed. No unauthenticated
data exposure, credential recovery or remote exploit is claimed.

Frozen SDK semantics were read at Go1.26.7 `net/http/request.go:1138` (multiple
Host rejection and URL authority precedence) and `net/http/server.go:1012`
(required/malformed Host). Actual process observations substantiate this finding.

## Full-layer conclusions

The constructor retains a single supplied `Arc<Store>`, explicit caller-owned
key and embedding snapshot; it does not open another DB, resolve credentials,
launch workers or cut over the CLI. Default direct writes are disabled. Listener
admission is loopback only. The owner applies bounded connections, headers and
body collection; signature/issuer/expiry and revocation checks remain mandatory;
role lookup failures deny writes. The shared save/delete adapter preserves the
existing CLI kind/provenance/governance behavior and HTTP's kind-empty/source
contract, autocommit sequence, audit gating and feedback ordering. Delete service
rechecks hydration admission under its Store lock. Retrieval/Get admission
precedes access feedback, avoiding partial history updates on unsafe data.

The existing HTML is preserved, CSS differs only in removed blank lines and the
two JS changes address actual public-status authentication and nested-search
defects. Rendering escapes inserted text and quoted attributes. Locked HTTP and
cryptographic crates are reused; the sole newly locked package is `httpdate
1.0.3`, with the existing registry/checksum provenance. No new agent capability
or credential store is introduced. All changed production modules/assets are
below the 400-line bound. The normal Store parent and complete serve fallback
remain intact.

Permanent comparisons inspect status, selected headers and parsed payloads;
raw bytes remain retained. Narrow ID/time/version normalizers check UUID and
clock/identity invariants. Complete wire/header-order/chunking parity is not
claimed. Unsupported native states are explicitly separate 501/no-write checks,
and the desired same-origin divergence is separate from Go parity. Original Go
403 and frontend defects, earlier six-table inspection, first fixture failures
and the historical overlapping-default-port run retain their original labels.

The additive three-OS workflow uses native frozen Go1.26.7 and owned DOM/runtime
fixtures, with both genuine failed-replay controls and always-retained reports.
Windows console grouping/CTRL_BREAK is a test path, not Linux proof of Windows
runtime. The jsdom adapter explicitly supplies a modeled write Origin and does
not certify graphical rendering, browser Fetch/CSP or native Windows/macOS.

## Fresh verification and additional evidence

Fresh complete Cargo verification passes **320 tests / 0 failed / 0 ignored in
33 summaries**, combined Memory+CLI all-target/all-feature strict Clippy,
workspace fmt and all-workflow actionlint. Actual Cargo execution paths and
current executable hashes are bound in the companion receipt. The original
source-bound binaries were reused byte-exact; no fresh-compilation claim is made
for cached artifacts.

Fresh inherited gates pass **590 reads, 60 Sets, 16 Deletes, 13 delegated
boundaries and 10 callback/output pairs**. Both real read-process controls reject
all 32 affected seeded cases, with state unchanged; all three actual write
identity/audit/exit controls reject at their intended assertion. The default
11434 port was assigned exclusively to this reviewer and checked empty before
and after the complete gates. Wrapper source and actual executable hashes are
recorded by their unmodified runners; the runners dispose those temporary
wrapper binaries, so no retained-wrapper-byte claim is made.

Already fresh on the immutable candidate:

* Permanent HTTP gate: 52 actual pairs, seven 501 boundaries with every SQLite
  table/column/row/blob unchanged, and both actual wrong-key/missing-row controls
  rejected at their intended comparison. Exact source-bound archived native Go
  and owner bytes were reused during the compiler hold; no new Go executable was
  falsely attributed to this execution.
* Desired same-origin: 18 actual pairs with positive authenticated operations and
  hostile Origin/no-write controls; native DOM: 11 checks, original unchanged JS:
  five checks reproducing its two defects.
* Additional 25 real Go/native security/correctness pairs and two native-only
  complete-SQLite refusal checks: canonical/tampered/padded signatures, issuer,
  blank subject, Go-compatible arbitrary signed header/future iat, role and
  profile-lookup failure, required-profile denial, revocation across a restart,
  persistence failure plus immediate in-memory denial and typed body failures.
  `/tmp/symaira-memory763-independent-security-v2.{py,json,log}`. The first probe
  used an incorrect native example env-variable spelling; that initial script,
  report and failure log are preserved unchanged, and the corrected run uses
  the actual example's variable. That fixture failure is not a source defect.
* Eight actual raw transport pairs: auth/Host/CSRF precede giant declared bodies,
  incomplete body and conflicting lengths return 400, incomplete headers expire
  around five seconds, and all state stays unchanged. These original observations
  and their duplicate/missing Host differences remain retained separately in
  `/tmp/symaira-memory763-independent-transport.{py,json,log}`.
* Seven additional actual native DOM/JS/network checks inject hostile HTML into
  memories, rules and entity fields: list/search/rules/entities render literal
  text and quoted title attributes; no injected element, code or resource request
  appears. This is actual owned jsdom/network evidence, not graphical-browser
  security enforcement. `/tmp/symaira-memory763-independent-dom*`.
* Fresh inherited read/config gate: 590/590, actual source-bound immutable Go CLI
  and native CLI, all seeded application state unchanged. The runner internally
  performs Go schema reflection even with a prebuilt CLI; this small subprocess
  was discovered after completion and reported to the coordinator; further
  compiler work waited for explicit release. This read runner is not described
  as entirely build-free.

The original source/archive verifier passed: 167 candidate hashes, 2,438 frozen
archive files, all 459 original retained artifact bytes and gzip round trips,
526 actual ELF artifact paths/492 unique byte sequences and 33 actually executed
ordinary test binary bindings. Stale auxiliary artifacts are not relabeled as
executed tests or freshly compiled source. Primary native owner is SHA
`fceacc36e9a194ca801ee6fc7ad016d50610d531d3dbf4ab715807bf55e8f9e0`, CLI
`e61fb7c9b60b5286851c0c6c7882b31511beae7c4dd5b150586d29e1fe0d3a42`, retained
native-built frozen Go `6751384f201ee600f53d0097414250a4cd30320be6d7b09df5836b740106499b`
and original immutable Go CLI `a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41`.

The first receipt finalizer treated a cases array as an integer; its failed
assertion and exact script/log remain retained. The metadata-only correction
counts the existing array and verifies every actual passed count; no test,
process observation, fixture state or gate was rerun or rewritten.

Routine whitespace validation additionally finds trailing whitespace at
`scripts/memory-http-oracle/replay.py:43`; record/fix this small validation
cleanup in the correction rather than rewriting the frozen review candidate.
This is not an additional P2. The optional automated Security skill host tools
were unavailable; inspection/process verification here was manual and no hosted
scan/SARIF completion is claimed.

## Remaining gates and release boundary

This review does not complete #763 or admit `memory serve`. Config/key-reference
resolution/rotation, full ordered/null/duplicate JSON/JWT decoding, unrestricted
PII/extraction/governance and policy retrieval, stats/sync/relay, proxy/custom
CORS, static URL/Range/conditional behavior, complete idle/write/read timeout and
blocking-work cancellation, historical Store migrations and real native three-OS
CI remain pending. The approved unreachable-TUI retirement does not remove any
shipped command or Memory domain operation. The full CLI remains Go delegated.

The final receipt records clean source, stable actual binaries, no target
process users and no default11434 listener. The target and port are explicitly
**RELEASED to Root** after all review executions ended. No candidate source edit
or GitHub action occurred. A focused corrected successor needs fresh review
before publication; the present candidate is not approved.
