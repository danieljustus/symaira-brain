# Memory763 initial reachability and product recommendation

Base/main e3dbda6cbb95429237d15a9b176209b7d107792c, isolated worktree `/workspace/symaira-memory763-ui`, branch `issue/763-memory-ui-inventory`. Frozen Go source dcddcef0df5789123c7c9a7ebe6e01f10e941f2c; approved existing actual oracle `/workspace/oracles/symbrain-go-dcddcef0`, SHA a68dce5b6f34d10ed568d2a89fab880c889e5ff578735c7bf2ad0535285eda41 (GoSDK1.26.7). No Rust target build/execution, repository edit, Go source edit, GitHub write or closure.

## Actual reachability

| Surface | Concrete path | Observation |
| --- | --- | --- |
| Embedded memory Web Console | `main.run → cmdMemoryWithFormat("serve") → cmdMemoryServe → buildMemoryHTTPServer → StartHTTPServer → httpMux → handleStatic → web.IndexHTML/StaticFS` | Real owned CLI listener delivers `/`, `/style.css`, `/app.js` with200 and exactly frozen embedded asset bytes. Explicit process only, fixed127.0.0.1 bind. |
| Web API dependencies | Same listener: list/search/set/delete/rules/entities | Actual synthetic bearer requests list/filter/search/create/delete successfully; no/invalid-token list is401, nonloopback Host root is403. |
| Embedded MCP/gateway | `symbrain mcp` and deprecated `serve` alias construct Memory Server and expose MCP | No call to StartHTTPServer on this route; constructor alone does not expose web. |
| Memory TUI | `internal/memory/tui.RunDashboard/InitialModel`, own package tests | No production import or call anywhere in root Go command tree. Actual `go list -deps ./cmd/symbrain` includes web, excludes tui. Real CLI rejects memory tui/ui/web/dashboard/--tui and top-level tui/ui/web/dashboard with exit2; memory help lists only eight existing verbs. |

13 actual CLI invocations and17 actual HTTP requests completed in disposable owned HOME/XDG/CWD/SQLite; server SIGTERM exited0. One actual owned Ollama fixture request provided a synthetic768-vector, so search never consulted an operator service. All credentials are synthetic. Final raw receipt `/tmp/symaira-memory763-ui-oracle-complete/receipt.json` includes full request/response bytes, commands/exits/stdout/stderr, source manifest and actual binary SHA. Dependency list `/tmp/symaira-memory763-ui-go-command-dependencies.txt` was produced by actual immutable Go package graph with `-mod=readonly`. No empty/skipped/crosscompiled case is claimed evidence. No visual/browser/native parity test has run.

Two first private harness runs stopped on Python setup assumptions (shadowing the http module and treating Go's empty list `null` as iterable); their scripts/logs/owned roots are retained. The final actual process replay handles Go's null arrays and passed all stated observations. These are probe setup failures, not product bugs.

## Value and recommendation to Root

**Keep and port the reachable Web Console; explicitly retire the unreachable legacy TUI implementation/port commitment rather than invent a new Ratatui command.** The web surface provides portable local administration on Linux/Windows as well as macOS: browse/filter/search, create/delete memories, rules and entity inspection against the same Brain-owned store. The existing SwiftUI MemoryView supplies browse/search/create/delete/rules/query/audit through CLI but is macOS-specific and is not a proven complete replacement for this portable console/entity view. Product contract PB-2026-09-09 accepts Brain administration GUI+CLI; no old no-GUI restriction supports arbitrary retirement.

A small compatibility port should retain the three existing assets as Brain-owned embedded resources and serve them through the same native memory HTTP owner being ported under the HTTP/MCP work. Reuse the typed API/store, loopback/policy/auth/CSRF/security headers and bounded lifetime; don't add a second store, autonomous listener, frontend framework or Browser worker. Native integration must wait for tested HTTP/JWT/governed-write contracts, rather than introducing an unauthenticated asset-only listener or implying that a Go-delegating serve is native parity. The current main still sends memory serve/sync through Go fallback. No Ratatui dependency should be added solely to reproduce unreachable code.

The TUI source contains browse/search/filter/delete and staged candidate promotion/rejection actions, but those are not callable through a shipped Brain command. Their domain semantics must continue to live in existing storage/governance paths; this inventory does not authorize deleting such operations from the memory core. Document the TUI as an unconnected consolidation remnant: no command or user workflow is being removed now, no terminal client replacement is claimed, and frozen Go remains intact until normal Go retirement. Update Phase10.8 and a dedicated pending/retired matrix row plus a release-note entry only after Root records its explicit product decision.

## Existing web defects to preserve as findings, not parity claims

- The actual status endpoint is public: `/api/status` with an invalid bearer still200, while `/api/list` rejects it401. Existing app.js checks status and can falsely label the user authenticated. A supported native console should verify a protected read; do not describe current status as an auth check.
- Actual nonempty search returns records shaped `{memory: {...}, similarity_score: ...}`. app.js currently reads `m.id/m.content/m.score` at the outer level. Its `m.id.substring` therefore does not conform to the proven API payload. The reachable search API itself works. Static code and actual wire shape prove the mismatch; no browser execution claim is made.
- Go HTTP `/api/set` accepts its legacy content/scope/metadata shape without a CLI kind field. This is distinct from governed CLI writes. The native HTTP/API owner must decide and test the write-safety contract; merely copying the frontend is not proof of a safe native cutover.

Do not silently freeze these mistakes as desired UI behavior or use them as a reason to remove the whole console. Preserve originals and make supported auth/search correctness changes explicit, with behavioral tests. Browser/DOM verification can be headless and owned; no operator browser profile is needed.

## Bounded acceptance plan after decision/resources

1. Root-approved ADR: keep portable web, no new unreachable TUI port, exact affected surfaces and migration/release note; ledger row for actual web asset/API contracts remains pending until tested native execution.
2. Port resource embedding and route integration in the native HTTP work, retaining `/`, asset paths/content types and host/CSRF/auth boundaries; do not complete HTTP in a separate ungoverned UI layer.
3. Compare real frozen Go vs native CLI listeners in owned roots for bytes/paths/errors/assets/read/write/scope/search/rules/entities/invalid or expired auth/denial/lifecycle. Verify corrected auth/search UI behavior through actual headless DOM/browser or an equivalent owned frontend runtime, including rejected writes and errors. Report any approved deviation concretely.
4. Genuine required native CI/platform gates and independent review; only then remove this reachable Go fallback and consider #763 completion. Initial Go-only reachability proof does not meet final Rust parity acceptance.
