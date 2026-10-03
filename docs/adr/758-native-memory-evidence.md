# Native memory evidence and schema repair (#758, #649)

Status: implemented increment; native Linux/macOS/Windows acceptance pending.

## Decision

Keep grounded-evidence ownership inside `symbrain-memory`. CoreKit's frozen
Go `evidencekit` remains the source-bound oracle, rather than introducing a new
shared Rust package for Brain's single consumer. Extraction, activity promotion
and consolidation can share the local `Extraction`, UTF-8 byte `Span`, strict
validator and same-transaction save/reparent operations.

Preserve the observable Go contract: exact alignment precedes normalized
whitespace, then fuzzy alignment. Equal fuzzy scores keep the first shorter
window and the earliest position. Fuzzy results are rejected by default during
persistence. Validation retains Go's error priority and its acceptance of
zero-length spans and unrecognized future status strings; changing those rules
would require a separately documented compatibility decision. The `char_start`
and `char_end` field names represent **UTF-8 bytes**, not Unicode character
counts. JSONL encoding retains Go's compact fields, HTML escaping, omitted empty
attributes and sorted attribute keys.

Make schema creation, actual-column repair, indexes and migration bookkeeping
one SQLite transaction. Journal-mode configuration stays outside because SQLite
forbids changing it inside a transaction. Applied migration names never replace
inspection of the real `memories` columns. This keeps the idempotent #649 repair
while preventing a later DDL failure from committing a partly repaired schema
or new applied entries. The write implementation moved into a focused file;
its stored vectors, hashes, identifiers and data model remain unchanged.

## Why

Evidence persistence must participate in the memory transaction. A detached
sidecar or independent database would lose source relationships when imports or
consolidation roll back. Matching the immutable algorithm and serialization
lets existing Go-derived evidence remain usable after cutover. Local ownership
also avoids a permanent dependency/API obligation for one application.

Schema repair responds to the reported store whose migration table claims all
versions while five columns are missing. Rechecking columns repairs those stores
without deleting data. Atomic DDL and bookkeeping avoid recreating the same
class of discrepancy after a later initialization failure.

## Verified evidence and remaining scope

The supplemental oracle executes immutable Brain revision
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` and verifies the exact CoreKit v0.17.0
source hash. It supplies 32 alignment cases, 48 validation cases, three complete
JSONL records and their SHA-256. Four existing production Go DB tests exercise
strict persistence, cascading deletion and transactional reparenting in isolated
HOME/XDG roots. The Linux gate passes 41 affected memory tests and 291 CLI/gateway consumer
tests, with no failures or ignored cases; formatting and strict Clippy pass.
Rust verifies those algorithm/byte fixtures plus rollback,
reparenting, missing-memory rejection, cascading deletion and the five-column
#649 reproduction. Three real replay controls reject a missing fixture, changed
alignment status and removed case with nonzero exits and the intended reason.

The initial consumer-test invocation picked an unrelated system `go` executable
and failed while building its fake MCP child. That result is retained separately;
acceptance uses the explicitly selected Go 1.26.7 SDK.

This increment does not remove any memory CLI fallback route. Full dynamic
configuration, command flags/error text, governed writes and Go-compatible JSONL
**decoding** still require their own source-bound implementation and native
acceptance. Unbounded fuzzy work must remain subject to the ingestion owner's
input budget; this compatibility helper does not claim a new resource limit.
`memory serve` and `memory sync` remain later work. #758 stays open. #649 also
stays open until the repaired native store is shipped and release evidence
confirms users receive it; its Doctor schema diagnostic remains outstanding.
