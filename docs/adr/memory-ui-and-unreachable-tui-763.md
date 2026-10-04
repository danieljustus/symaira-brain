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
It includes13 actual CLI invocations,17 actual HTTP requests, exact asset bytes,
the actual Go dependency graph, synthetic SQLite state, raw logs, source hashes
and the original recommendation. Everything ran in disposable HOME/XDG roots
with a synthetic JWT and owned local embedding peer. Go exited0 on SIGTERM.
This is Go reachability evidence, not native parity or browser execution.

Two inherited frontend errors must be corrected explicitly in the native copy.
The public `/api/status` returns200 even with an invalid token, while a protected
list returns401; it cannot establish authentication. Verify a protected read
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
