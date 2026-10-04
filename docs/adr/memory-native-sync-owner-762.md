# Native Memory sync stays on the embedded Memory owner

Status: source-stage decision and implementation checkpoint for #762;
compilation, process parity, independent review and native three-OS CI pending.
User authority delegates implementation/product decisions and their reasons.
PB-2026-09-09 revision4, not historical hosted-sync marketing text, controls
the product boundary.

The reachable frozen command is `symbrain memory sync --remote <url> [flags]`.
It is an existing in-process client of the Memory HTTP protocol, not a new
hosted service, account product, scheduler or separate database. Preserve this
useful portable-context operation. The source base is approved PR803 head
`6ae73a7eef3442e2b4211a56043941339fb5a1d6`, with main
`2b6d49f250a81650eda1b93cec7d859a171873bb` as an ancestor. The separately
prepared Store BUSY and historical-schema corrections are not silently
integrated or treated as proven here.

Keep cursor, oplog, LWW and relay data ownership in `symbrain-memory`, using
the existing `Store`, tables, triggers, audit helper and vector LSH. Sync must
not call the CLI governed-set pipeline: that pipeline creates a new ID,
redacts/enriches metadata and replaces timestamps/embedding. Go's sync ingests
peer identity/clocks and uses strict LWW. Conversely, sync deletion calls the
storage DeleteMemory contract, not CLI deletion's retrieval-feedback step.
No Core-to-Brain dependency, second store, new listener, Guard conduct policy
or capability exposure change is introduced.

Use a complete sync-specific Memory wire model. The existing public Rust
Memory is a lite convenience projection and would silently lose review,
temporal, actor, embedding, access, tier and evidence fields. Peer entities
and evidence are represented on the wire; frozen UpsertMemoryIfNewer does not
persist these arrays. Outgoing sync uses the frozen lite DB projection without
public list visibility filtering or entity/embedding hydration. Only the
literal metadata `sync_exclude` string `"true"` excludes rows.

Pull memories before tombstones; push from the original cursor in independent
200-row memory/delete keysets. Do not wrap the run in a new transaction or
add automatic backoff/retries. Earlier applied rows remain after a later
failure and the cursor is saved only after success. A subsequent invocation
replays from the prior cursor with tombstone-aware LWW. Deadline enforcement
belongs at the transport requests, including an earlier caller deadline; an
expired push with no outgoing rows can still complete in frozen Go.

Keep transport and relay cryptography explicit owner interfaces while source
work is restricted. This checkpoint contains the native Store and orchestration
implementation plus a canonical model codec; it is not a complete sync port.
The HTTP client/Go URL parser, ordered typed JSON decoder, request encoder,
base64 codec and native relay AES backend remain unimplemented admission
requirements. In particular, canonical Serde is not equivalent to Go's folded
field names, ordered duplicate merge, null retention, depth/type errors or
first-object decoding. No stub or identity crypto codec can qualify as parity.

For the concrete relay backend, use reviewed, pinned AES-GCM/PBKDF2-SHA256
primitives and owned cross-runtime tests; do not invent cryptography. Preserve
Go's 600000 iterations, v1 salt16/nonce12/tag16 layout, legacy salt8 layout,
v1-authentication-failure legacy fallback and run-scoped salt/key cache. An
ephemeral relay passphrase is not a Vault master key or durable credential
store. It must not be logged, echoed in normal result JSON or persisted.
The gateway continues to delegate credential management to separate Vault.

Preserve and explicitly test the following source-derived Go quirks before
deciding any separate behavior correction. They are not runtime findings or
newly accepted native contracts at this checkpoint:

- HTTP status300 is accepted; success/error decoders read only the first JSON
  value through10MiB/64KiB limits. HTTPS opaque/no-host URL acceptance can fail
  later during request construction. CLI insecure override warns even on HTTPS.
- Relay push-only always performs the relay pull first. Recognized but LWW-
  skipped payloads count as fetched; delete wins when both inner fields exist.
  Go does not bind outer blob ID/clock to inner content. Relay pagination uses
  a strict timestamp, with an inherited equal-timestamp page-boundary risk.
- Final cursor is max(remote clock, sent local clock), excluding its previous
  value; an empty run can reset it and a lagging peer can regress it.
- The continuation branch of GetDeletedSinceCursorID lacks `op='delete'`, so
  after a full tombstone page it can expose latest upsert events as deletes.
  Native source retains this branch solely for future actual comparison. This
  destructive family must not be admitted without an explicit reviewed
  decision; do not call it a benign normalization or blanket parity exception.
- Upsert updates exclude created identity, valid_from/valid_to/superseded_by;
  equal upsert loses, equal tombstone wins. Audit failures are intentionally
  ignored by the frozen storage contract. Invalid wire NULL memory pointers
  can panic in Go; do not reproduce that crash or silently accept the family.

Real `memory sync` remains Go-gated and HTTP sync refusals stay closed. Issue
#762's request to remove `memory_cli::requires_go_fallback` entirely cannot
remove unrelated serve/config/governed-write gates: these share the predicate
and need their own proof. First prove the complete sync seam, then remove only
its routing condition; remove the predicate itself when every remaining owner
has independently completed its acceptance. #762 remains open.

`scripts/memory-sync-oracle/cases.json`, `CONTRACT.md` and the supplemental
actual-Go probe define the next validation. No compiler, Go/native process,
listener, crypto operation or target was used in this source-only stage.
Prepared regressions and source/hash/format checks are not acceptance evidence.
