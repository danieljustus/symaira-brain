# Keep the portable Memory Console and retire the unreachable TUI plan

Status: accepted product decision, 2026-10-04; native implementation and runtime
cutover gates pending. Refs #763 and #731. The user delegated long-term product
decisions to the coordinating maintainer, who explicitly approved this decision.
No additional maintainer confirmation is required.

The embedded Go Memory Console is reachable through `symbrain memory serve`.
The actual command starts the loopback HTTP owner, which serves `/`, `/style.css`
and `/app.js` and the memory API. It supplies useful portable administration:
browse/filter/search, create/delete, procedural rules and entity inspection.
Keep and port this surface. The accepted Brain GUI/CLI target in
[PB-2026-09-09](0002-product-boundaries.md) applies; macOS SwiftUI alone is not a
verified portable replacement for these workflows.

The Go `internal/memory/tui` package has no production importer or caller in the
Brain command tree. The actual command dependency graph includes `memory/web`
and excludes `memory/tui`. Actual `memory tui/ui/web/dashboard/--tui` and top-level
`tui/ui/web/dashboard` invocations return usage errors. Retire the unconnected
legacy TUI implementation and mandatory Ratatui port commitment. No shipped
command is removed and no new terminal client is promised. Candidate promotion,
rejection, filtering and other domain operations remain owned by memory and its
governance interfaces; this decision does not remove them. Frozen Go stays
unchanged until its separately verified retirement under #783.

The original source-bound inventory is retained under
[go-reachability-e3](../../migration/evidence/memory-ui-763/go-reachability-e3/README.md).
It includes 13 actual CLI invocations, 17 actual HTTP requests, exact asset bytes,
the actual Go dependency graph, synthetic SQLite state, raw logs, source hashes
and the original recommendation. Everything ran in disposable HOME/XDG roots
with a synthetic JWT and owned local embedding peer. Go exited 0 on SIGTERM.
This is Go reachability evidence, not native parity or browser execution.

Two inherited frontend errors must be corrected explicitly in the native copy.
The public `/api/status` returns 200 even with an invalid token, while a protected
list returns 401; it cannot establish authentication. Verify a protected read
instead. Search returns `{memory: {...}, similarity_score: ...}` records, while
the old JavaScript expects outer `id/content/score`; consume the real nested
shape. Preserve original source and actual payloads; test the corrected native
frontend in an owned DOM/browser runtime. Do not alter frozen Go or label these
defects desired parity.

The web port belongs to the shared native memory HTTP owner and existing Store.
It must retain loopback binding, Host/CSRF/auth/profile roles, body and header
bounds, JWT revocation and secret handling, redaction, timeouts and graceful
shutdown. Do not create an unprotected asset-only listener, a second store or
an automatic background worker. Human UI does not expand agent exposure or put
Vault master keys into Brain. Existing HTTP writes and governed CLI writes are
different contracts: port and prove the HTTP owner's safety and persistence
sequence rather than substituting an unrestricted CRUD shortcut.

`memory serve` advertises API/sync-peer behavior beyond the UI. Native resources
or a positive UI slice alone do not justify admitting that complete command.
Keep fallback until all admitted routes/configuration/state contracts have real
Go/native process evidence, or record a separately approved precise deviation.
Record unsupported routes and configurations explicitly. Native three-platform
CI and independent review remain required before merge or full #763 completion.

Use the existing Brain-owned assets and typed API instead of a new frontend
framework. Reuse locked HTTP/cryptographic libraries when needed, justify direct
dependency edges, and retain a single lifecycle/store owner. Pixel parity is not
the gate; working authentication, queries, writes, errors and safe shutdown are.

## Bounded native owner before CLI admission

The first production increment adds `symbrain_memory::http::Server`, built on
Hyper's existing HTTP/1 parser/runtime and the existing `Arc<Store>`. No native
Memory HTTP owner previously existed: the Go command owns thirteen API routes,
including sync/relay, so a static-assets port would silently lose advertised
operations. The explicit constructor accepts a caller-owned key and embedding
runtime snapshot; it opens no second database, resolves no Vault reference,
generates no signing/master key and starts no automatic worker or browser.
Writes are disabled by default. An actual example invokes this production owner
for owned process evidence; it is not a replacement CLI command or a native
`memory serve` admission claim.

The positive boundary includes original HTML, semantically identical CSS with
blank lines removed to keep the asset below the repository's 400-line limit,
and the two narrowly corrected JavaScript behaviors. The same owner enforces
loopback-only listener admission, bounded connections/body/header handling,
Host/CSRF/CORS, signature/issuer/expiry, SQLite profile roles and persistent plus
in-memory JWT revocation. It serves UI reads and explicit conservative direct
writes through existing Store methods. Shared write adapters preserve CLI kind
and provenance while HTTP uses its actual legacy kind-empty/source-tool-HTTP
contract. The governed CLI requirement for an explicit kind is unchanged.
Read admission precedes retrieval/Get access feedback, so rejected unsafe data
cannot partially mutate history. Unsupported policies, extraction/PII writes,
working/session/entity writes and unported routes fail explicitly with 501.

Dependencies already pinned in the workspace are reused directly. Enabling the
existing Hyper server feature adds exactly one previously absent package:
`httpdate 1.0.3`, checksum
`df3b46402a9d5adb4c86a0cf463f42e19994e3ee891101b1841f30a545cb49a9`,
for Hyper's HTTP Date handling. Existing package versions/sources/checksums are
unchanged; Tokio signal support reuses the already locked signal-hook-registry.
This avoids a new web framework, custom HTTP parser and Ratatui dependency.
The optional jsdom runtime is an owned test tool, not a product dependency.

Fresh complete Go-created databases are the proved state boundary. The
coordinator's separate actual Doctor inventory identifies historical rules
`updated_at` backfill, entity-relation UUID/time upgrades and Porter FTS rebuild
as still unproven on this parent Store. This owner does not repair or claim all
legacy schemas; #649/#758 remain the store-history owner. No duplicate migration
or private API database is added.

The remaining complete-command gates include configuration/credential-source
resolution and key rotation, stats/sync/relay, full typed ordered/null/duplicate
JSON and JWT decoding, policy/client/profile-specific retrieval, unrestricted
redaction/extraction/governed writes, trusted proxy/custom CORS configuration,
URL/path and static Range/conditional behavior, complete read/write/idle timeout
semantics, blocking-work cancellation and native Windows/macOS runtime evidence.
The positive owner slice cannot bypass these gates: the entire existing
`memory serve` route remains delegated. The actual process and DOM runner,
negative executions and exact retained limitations live in
[scripts/memory-http-oracle](../../scripts/memory-http-oracle/README.md).

A retained first final replay exposed a bad test expectation that HTTP creation
and update times must be equal. Actual Go Prepare/Save observes them separately.
The shared save adapter now takes the same two clock observations; validation
checks the actual `valid_from <= created_at <= updated_at` ordering. Original
raw pairs and the failing equality assertion are retained rather than rewritten.
HTTP Get also uses the existing nanosecond SQLite timestamp renderer, consistent
with the shared direct-delete service. These are explicit clock/state contracts;
independent process timestamps are not asserted byte-equal.

The additive HTTP workflow runs the actual owner, freshly native-built frozen
Go and the owned DOM suite on Ubuntu, macOS and Windows. Windows shutdown uses
an explicitly owned console process group and CTRL_BREAK; Go's runtime maps
that event to Interrupt, and the native seam explicitly handles it. Linux
execution does not prove that platform path. The CI workflow retains every raw
receipt and fails both wrong-key and missing-row process controls. All native
three-OS workflow outcomes remain pending until those jobs execute. Before this
portable-runner successor, all 523 existing ELF artifact paths (489 unique
byte sequences, including the actual source813 CLI and owner) were compressed,
SHA-verified and round-tripped into the owned oracle archive; earlier receipts
and binaries keep their original source attribution.

## Exact same-origin correction

The coordinating maintainer also approved a focused third native correction
after an actual frozen-Go/native original pair: authenticated POST with
`Origin: http://127.0.0.1:<listener>` returned 403 and performed no write in both
old owners. The original extension-only CORS policy prevents normal local
browser-equivalent writes. Preserve that original pair under
[same-origin-original-9b](../../migration/evidence/memory-ui-763/same-origin-original-9b/preservation.json).
This is evidence from supplied Origin headers, not a graphical browser request.

The native owner admits only the exact `http://` validated loopback Host
authority and the actual listener port, which the lifecycle owner records before
accepting connections. It does not infer a foreign authority, permit HTTPS or
arbitrary localhost ports, accept credentialed/prefix/null origins, weaken JWT
or grant a write role. Existing Host/CSRF/JWT/profile checks remain in sequence.
A separately recorded desired-behavior suite proves OPTIONS and real
authenticated POST/search/delete plus hostile Origin controls; this divergence
is never normalized into Go parity. The existing full serve fallback is unchanged.
The owned DOM adapter now supplies browser-equivalent write Origin headers
explicitly and labels that modeling limit; it is not a browser Fetch/CSP proof.

## Final bounded Linux validation

The [source-bound final evidence](../../migration/evidence/memory-ui-763/native-owner-824/README.md)
records 52 actual HTTP comparisons, seven complete-SQLite no-write boundaries,
two real negative executions, 18 explicit same-origin desired-behavior pairs and
the owned DOM/original-defect checks. The inherited shared CLI regression gates
also pass: 590 reads, 60 Sets, 16 Deletes, 13 delegated boundaries, ten output/
failure pairs and three actual mutation controls. The final default-port run was
exclusive; the earlier overlapping run remains preserved as historical evidence.

Production sourceD1 passes 320 affected ordinary tests and strict all-targets/
all-features Clippy, fmt and workflow lint. The final824 successors strengthen
test inspection only; production/assets/Cargo/workflow bytes remain identical.
Earlier six-table no-write receipts keep that narrower attribution. Final824
records all SQLite columns, rows and blob bytes, including sync and FTS tables.
Original failures and source-bound binaries are retained and hash-verified.
This completes the bounded Linux owner proof; full serve admission, native
Windows/macOS acceptance and independent review remain required.
