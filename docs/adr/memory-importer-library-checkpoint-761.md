# Memory importer library source checkpoint (#761)

Status: source implementation and prepared evidence, **not runtime acceptance**.
Decision authority is the user's delegated long-term product direction under
PB-2026-09-09. The frozen Go reference remains unchanged. No route is admitted,
provider contacted, credential resolved or importer run by this checkpoint.

## Ownership and reachability

The frozen `cmd/symbrain/cmd_memory.go` dispatches list, search, set, delete,
rules, query-log, sync and serve. It has no import command. Source-wide import
and symbol searches find all importer constructors and `Registry.NewRegistry`
without non-test production callers; `internal/memory/discovery` consumes the
DTO interface, but its Scan function also has no production caller. These are
existing library contracts. They are useful to retain for curated agent
context and future explicit import workflows; this does not justify inventing
an automatic importer, background capture or a new CLI entry point now.

Activity search/get/status already have native Store and CLI/MCP owners. Reuse
them. Memory803 prerequisite a30bba8bbd52323fa5e000207f1c391d576ac86c was normally
merged onto main5e232700bb9031fc34d4995465a4037837540abb; all ancestor original
proofs remain tracked. There is one Store/connection and no new service, LLM
client, master key, credential owner or Brain conduct-policy layer.

## This source slice

Implement the five local adapters together because they share byte Markdown,
line-scanner and read-only file contracts: Codex Memory, Curated Memory, Aider,
Shell History and Obsidian. Raw content, metadata and session IDs are bytes;
paths remain PathBuf. JSON replacement is limited to Go's JSON-string-array
and metadata rendering boundaries, never to path lookup or content ownership.
`GroundedFact.to_extraction` delegates to the existing evidence representation
and refuses invalid UTF-8/oversized spans rather than replacing source bytes.
The optional transcript interface is represented by presence plus value: the
Go registry checks interface presence even when its method returns false.

Durable import markers use the shared Store's import_state table. The source
trace deliberately retains the nonempty GetLastImportTime error: modernc1.59
returns an expression MAX(imported_at) as TEXT without a declared DATETIME;
CoreKit0.17 sqlitekit's DSN does not enable _texttotime; database/sql.NullTime
cannot scan a string. This is a **source-derived** contract, not an executed
Go observation yet. The prepared actual Store oracle must test this trap
before any cursor-based registry admission. Do not silently parse it into a
successful native cursor or describe the library registry as operational.

Activity Expire, ClearTimeRange and ClearAll now use the existing connection,
delete episodes before segments transactionally, retain committed counts on
cleanup failure and checkpoint/vacuum/check WAL afterwards. Expire skips
cleanup on zero deletions, while explicit clears still clean an empty store.
The WAL path comes from the exact SQLite connection, not a second database.
Configured raw path versus SQLite's reported filename, rename/symlink history,
busy checkpoint and post-commit cleanup error behavior still need actual
process probes before exposing these mutations.

## Deliberately retained source quirks

- Codex Memory truncates at 1 MiB, may cut UTF-8, applies deny before allow,
  suppresses 10min files covered by any eligible old 6h parent across extension
  boundaries, ignores instructions.md and uses byte evidence spans.
- Codex's frontmatter closing marker is a substring `\n---`; Curated Memory
  and Obsidian require an exact Scanner line. Repeated keys retain source
  overwrite/list behavior; this is not a general YAML engine.
- Curated Memory discovers lowercase .md beyond memory directories, skips its
  specific MEMORY.md indexes, truncates at 5000 bytes, records the session ID
  as file_path, and stats after parsing instead of trusting discovery time.
- Aider ignores Scanner.Err and flushes an already scanned assistant prefix.
  Its role marker's inline text is dropped. Content length is bytes.
- Shell History ignores success_only/duration, retains duplicate epoch IDs,
  and does not associate a Bash #timestamp line with the next command.
- Obsidian ignores its folder constructor option, preserves spaces inside
  wikilinks, filters folders/tags as the source does, and may import an empty
  body with its Note/title prefix.

These behaviors are not quietly repaired during a parity port. Domain
improvements require a separately documented decision and actual tests.

## Remaining gates and work

No compiler/Cargo/SDK probe/product process, target/cache, provider, port or new native CI was
allocated or used here. The 24 unit functions, 70 constructor recipes, three
real-process input controls and Store/retention process plans are **prepared**.
Source formatting/AST/shell/workflow and hash checks are the only validation
available at this stage. Native three-OS CI and different-author review are
mandatory before acceptance; a prepared fixture is not a passing observation.

All non-Unix lexical/default-HOME/raw UTF-16 contracts are explicitly refused
in the path helper pending a proper native Windows port. Unix joins clean
lexically before kernel lookup and never canonicalize symlinks. Non-UTC local
timestamps, chrono's narrower epoch domain, raw typed I/O diagnostics and
error-before/after-partial-result precedence remain actual-proof gates; ordinary
Rust OS error text is not claimed as Go-compatible byte diagnostics.

The ten other source families remain unported: Calendar, Claude Code, Codex
JSONL, Email, Git, GitHub, Hermes, Memory Tool, OpenCode and Paperless. Memory
Tool includes Mem0, OpenMemory, ChatGPT, SessionAdapter and SmartImporter.
ChatGPT's secure Unix descriptor-relative nofollow/nonblock read, regular-file
and 64 MiB checks, depth16/entry10000 bounds and substituted-link/FIFO controls
must survive its own future port; none is relaxed by these other adapters.

Registry extraction/summarization, untrusted-content sanitization, redaction,
staged/private/provenance metadata, embeddings, partial-commit/state semantics,
privacy/category selection and map-order diagnostics remain pending. Coordinate
with #760's engine owner and reuse its domain APIs. Activity upserts, rollups,
staged episode/evidence promotion, Get/List typed errors and provenance-based
deletion also remain pending. No observation automatically becomes a live
memory, no source document is made Brain's editable source of truth, and no
email/post/network operation is authorized by this implementation.
