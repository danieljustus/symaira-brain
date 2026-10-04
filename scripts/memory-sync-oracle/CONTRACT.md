# Frozen sync contract inventory (source only)

Oracle: `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, pinned Go1.26.7.
No observations or successful runtime counts are asserted here.

| Seam | Frozen owner | Native source / remaining proof |
|---|---|---|
| Command tree/flags/output | cmd/symbrain/cmd_memory_sync.go, main dispatch, normalizeFlags, extractFormat | CLI policy stays gated; exact argv/exit/stdout/stderr/warning and failed-output committed state pending |
| URL/auth/request/body | syncclient/client.go | SyncTransport interface only; synchronous URL guard, redirects/TLS/proxy/headers/timeouts and byte-exact encode/decode pending |
| Counters/clock/retry | syncclient/runner.go | sync/runner.rs,push.rs,relay.rs; actual request/order/state proof pending |
| Cursor identity/storage | db/sync.go | sync/store.rs; raw remote string, missing zero, DB failure, persist-last pending |
| Memory wire/full schema | db/memory.go Memory and scanMemoryLite | sync/model.rs,time.rs; canonical only, exact typed ordered decoder/float/time/null bytes pending |
| Query projection/filter/keysets | db/memory.go getMemoriesSinceCursorID | sync/store.rs; no list filters, sync_exclude, 200/201 ties, malformed SQL types pending |
| Row LWW/tombstone/audit | db/sync_oplog.go, db/memory.go UpsertMemoryIfNewer/DeleteMemory | sync/upsert.rs,store.rs; same connection/triggers/audit, callbacks and race boundaries pending |
| Relay storage/server auth | db/sync_oplog.go, mcp/http_server.go, handlers.go | Existing HTTP owner integration remains closed; no second server/501 change |
| Relay encryption | security/crypto.go | RelayCodec interface only; interoperable authenticated concrete backend pending |
| Config/database adoption | memory/config, db.Open, PR803 | Approved current-store base; pending BUSY and historical-schema work separate, no blanket legacy acceptance |

CLI order is output-format extraction, flag normalization/parse, first extra
positional, required remote, relay passphrase CLI/environment, URL guard,
insecure warning, direction default, token/passphrase environment, config load
(failure defaults), DB override/open, outer timeout, Run, checked render.
Empty CLI token/passphrase fall through to environment. Both directions true
are allowed; both false at public Run is an error but CLI defaults both true.

Run validates remote, nonnil DB, directions, relay passphrase, URL before cursor
read; its own nonpositive timeout defaults60s under the caller context. Plain
pull respects opaque next_cursor over since, upserts memories then deletes,
and observes even skipped timestamps. Push reads each keyset independently,
does not send empty batches, counts server applied/deleted only, and advances
local clock from all sent rows. Relay pulls even in push-only, counts recognized
inner payloads even skipped, delete takes precedence over memory, and ignores
empty payloads. Cursor write is final; no wrapper transaction/retry is present.

HTTP bodies need Go declaration order, sorted metadata keys, HTML escaping,
nil arrays as null, omitted zero fields, base64 blob encoding, float32/64
representations and RFC3339Nano (plain since truncates subsecond precision).
Go ApplyResult canonical snake key wins even null/bad type; decoding integer
errors are ignored to zero. Standard struct fields use folded names and ordered
duplicate mutation; arrays/slices and null use Go semantics. Use raw typed
decoding, not a Value map that erases duplicates. Depth, nonUTF8, leading BOM,
surrogate, overflow and error strings require actual frozen public client runs.

Go server changes has authenticated read ownership; apply and relay use
read-write auth. Preserve method checks, bounded request decoding, UUID/scope
validation, policy filter/redaction, compatibility counter aliases, sync audit,
relay LWW and all upstream loopback/Host/JWT/CSRF checks in the existing HTTP
owner. Client parity by fake server cannot prove these server protections.

Each future comparison retains full raw requests/responses, both process
streams/status, all logical DB rows/types/binary bytes, FTS integrity, audit and
oplog event order, entity/evidence cascade and old/new cursor. Bind nondeterministic
clock/UUID values to concrete owned steps; do not drop tables or normalize away
unexpected requests, counters, cursor regressions or partial commits.

Existing Go tests remain immutable and all test names/hash paths are retained
in the static source manifest. New cases and controls must be additive. Preserve
the first genuine failing input/log/state before any corrective source change.
Native Windows/macOS execution and different-author review remain required.
