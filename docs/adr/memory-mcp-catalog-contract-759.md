# Memory MCP metadata stays with its embedded owner

The frozen Go implementation exposes Memory tools through `symbrain mcp`
(and its deprecated `serve` alias). `symbrain memory serve` owns the separate
HTTP synchronization/Web Console listener. Issue759's shorthand must not move
MCP into that HTTP owner or create a second Memory store.

The existing Rust Gateway already uses the shared Memory store, generic MCP
transport and profile policy. Actual baseline probes against frozen Go
dcddcef0 and approved native Memory publication27001b1 found the same sets of
tool names across six profiles, but different descriptions, titles, input
schemas, annotation hints and array order. These complete protocol values
matter to clients and to a separately installed Guard that pins schemas.
Name-only comparisons are insufficient for this contract.

We therefore retain the existing owners and port the eleven actually reachable
Memory/Activity descriptors, including Go registration order. Versioned static
metadata is compiled into the existing Gateway; it is not loaded from a user's
filesystem or generated using Go at runtime. Typed private descriptors retain
the complete ordered inputSchema as RawValue. Memory read tools do not acquire
the idempotent hint used by Activity tools. Existing profile evaluation selects
the names before serialization; allow/deny list order does not widen exposure.

The four candidate/promotion/rejection/query-log descriptors are not exposed by
the current Brain Memory policy universe even when explicitly requested. Their
existing conservative implementation remains separate; this change does not
silently enlarge the profile universe to expose them. Skills and foreign server
catalogs remain with their current owners.

The baseline raw streams, profile inputs, binaries' hashes and289 frozen Go
source hashes are retained in
`migration/evidence/memory-mcp-759/catalog-baseline`. An actual process gate
compares full initialize/catalog streams, field values and order, framing,
exit codes, diagnostics and profile exposure, including the deprecated alias.
Real title/schema/order/exposure mutant processes must be rejected. A dedicated
native Linux/macOS/Windows workflow retains its SDK and source receipts.

This is a bounded catalog correction. It does not claim complete tools/call,
legacy database, authorization, filesystem or HTTP parity, or close759/758.
The original baseline also records Go-created JWT-secret and audit files that
the native startup does not currently create; they are remaining lifecycle
contracts, not normalization allowances. Full platform acceptance, independent
review and protected checks remain required before merge.

## Fresh clean-source result

Source2f5a59d, normally based on approved Memory27001b1 and maine3dbda6c, passes32 complete actual Go/native process pairs across eight profiles, both framing modes and both command aliases. All five actual title/hint/schema/order/exposure mutant processes are rejected.316 gateway/MCP/CLI all-target/all-feature tests pass with zero failures or ignores (35 summaries); strict Clippy, formatting and Actionlint pass. The original MCP process suite, foreign-child routing, malformed frames and Unix SIGTERM behavior also pass.215 current source hashes and107 actual ELF paths/75 unique hashes were verified and retained before target release. First source855 Clippy failure and its passing32/4 process proof remain unchanged in the baseline evidence. Complete receipts are under `migration/evidence/memory-mcp-759/final-2f5a59`. Independent review and genuine native-three-OS acceptance are still pending; this is no closure or filesystem-parity claim.
