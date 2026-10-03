# Governed native CLI writes for Memory (#758)

Status: independent review closed the original governance/output findings and
requested a focused actual stdout SIGPIPE correction. Its independent review
and native three-OS CI remain pending. Full #758 remains open.

## Decision

Use a CLI-specific direct-write adapter on the existing Memory store. Keep the
MCP migration API unchanged: the CLI's frozen service pipeline has defaults,
entity resolution and audit behavior that the existing MCP port does not yet
reproduce. New native writes must persist the complete reachable service state,
not just print the same ID. No new shared CoreKit dependency is introduced.

Admit existing-store writes only when content and caller metadata bypass all
frozen Go PII patterns and every potential secondary-fact trigger. Admission is
conservative and currently limits content/metadata values to ASCII; Unicode
actors, metadata keys and entity names remain supported. Project provenance is
checked separately because a project directory name also enters PII redaction.
Invalid metadata JSON, including wrong values hidden by later duplicate keys,
retains Go. The typed decoder accepts null maps and null string values as Go does.
Raw non-UTF8 actors/entity arguments also retain Go rather than storing replacement
characters.

Staged writes bypass Go's conflict checker. Non-staged writes are admitted only
when the existing typed config snapshot actually disables it. File false is a
zero value ignored by configkit; an explicit nonempty environment false does
apply. The CLI's `--staged` flag remains authoritative: `memory.stage_writes_by_default`
does not stage CLI writes. This corrects the previous baseline's unconditional
native insert for default-enabled conflict checking. Deduplication, contradictory
facts, supersession and conflict audits remain on Go until ported and proven.

Persist provenance, trust and policy defaults, explicit metadata overrides,
project detection, configured embeddings/model/source, content hash, LSH bucket,
binary sign bits and every primary row column. Save an initially unclassified
approved row, record the set audit, link entities, then update canonical kind and
optional staging in the same order as Go. These updates must also produce the
same sync oplog sequence. Entity lookup uses name first and indexed alias second;
existing identities survive. New entities use distinct UUID v4 IDs, caller author
and entity-create audits. Resolution/write/audit failures follow Go's best-effort
boundary. Empty author stays empty; it must not silently become `mcp`.

Delete follows Go's service access-feedback update before deletion, then writes
the delete audit using the creator/session/scope observed after that update. A row
removed during access feedback still succeeds and produces no delete audit, as Go
does. A callback probe caught the earlier native adapter reading attribution too
soon; retain the actual differing audit rows as regression evidence. Hydration admission
refuses unknown schema, malformed typed JSON, noncanonical dates and non-UTF8
rows; validation repeats before mutation to avoid deleting a row changed after
routing. Reject leap seconds explicitly because Chrono accepts them while Go's
hydration rejects them. Exact corrupt-row/concurrent-error diagnostics remain a pending gate.
Go cascades evidence and query-result references, but its entity links and memory
associations have no foreign keys and survive deletion. Preserve that observable
contract; a cleanup policy needs a separate explicit decision and migration.
The initial delete harness incorrectly assumed all relationships cascaded; the
actual Go failure exposed this test assumption, which was corrected before acceptance.

New-file/directory mode and database-open error parity remains pending, so set/
delete on nonexistent stores retains Go. Read/search routes keep their previously
reviewed boundary. Hamming-prefilter retrieval, grounded secondary extraction,
PII redaction/logging/audits and evidence JSONL decoding are still incomplete.
`serve` and real synchronization stay delegated under #759/#762; this increment
does not certify #760/#761 MCP work or release-level #649 repair/Doctor acceptance.

## Evidence contract

Execute immutable Go `dcddcef0` and the real native CLI with disposable HOME,
USERPROFILE, XDG and cwd, no runtime PATH or native Go fallback. Embedding servers
are owned loopback fixtures; even invalid-config reset owns the default port
before the process starts. No operator credentials or paid endpoints are used.
Compare literal stdout/stderr and exit, every application table/column/row/blob,
retained legacy rows, aliases, entity relations, audits and sync events.

UUID substitution is limited to validated distinct generated v4 identities and
explicit relationship columns; the returned ID must identify exactly one primary
row. All generated timestamps retain literal receipts, must fall within the actual
process interval and obey prepare/save/governance ordering. Explicit timestamp
metadata overrides stay literal. Only the validated generated observed-at literal
is replaced in the original metadata JSON, preserving key order and escaping.
Physical FTS shadow blocks encode random IDs and depend on SQLite internals; keep
their full raw state and SHA receipts, compare the logical FTS projection and run
FTS integrity checks instead of claiming shadow-byte equality.

Actual executable controls corrupt the returned ID, audit actor and success exit
independently after running the real native candidate. The gate must reject each
for its intended reason and retain real process outputs/database state. Separate
fallback probes record the actual delegated Go process and require the native
reply to match those literal bytes and exit; they do not count as native writes.
Source/SDK/binary hashes and clean revision bind final receipts. Native three-OS
CI and independent review remain required; no issue is closed by this document.

## Retained Linux evidence

Clean source `14d24144df9fdf7e55db25b0ba64b1dcfa587468` includes the normally
integrated PR803 Windows borrow/SQLite cleanup corrections through `9c0ed065`.
Fresh gates pass 60 native set pairs, 16 native delete pairs, 13 actual delegated-Go
boundaries, three executable write controls, the full 590 baseline process pairs
and both baseline process controls. All 161 affected Rust tests pass: 117 CLI
unit tests, 32 Memory unit tests, four immutable-Go fixture tests, seven store
tests and one complete Activity CLI fixture test. Strict Clippy, format and
workflow lint pass. Eleven changed production Rust files are each below 400 lines.

Full raw state/transcripts, explicit identity/time bindings, source and binary
hashes, immutable Go source provenance, test logs and rejected prior controls are
tracked under
`migration/evidence/memory-cli-758/governed-writes-14d2414/verification.json`.
The evidence commit changes documentation and receipts only; these gates bind the
stated clean source and executable, not an untested later source change. Preserve
the first invalid FTS callback fixture separately from the corrected callback's
actual Go/native audit mismatch. No cleanup exception or broad state normalization
was used to turn either failure into a pass.

## Governance and output failure correction

Independent review of clean `14d24144` (publication `28e8e7ae`) found three
healthy SQLite callbacks that made native Set claim success after Go failed:
`RAISE(IGNORE)` on kind or review status, and deletion of the primary row by the
set-audit callback. SQLite successfully executing an UPDATE is insufficient;
both governance updates must affect a row. Return the frozen Go `memory not
found: <id>` error immediately when either update affects zero rows. Preserve
the insert, audit, entity links and any earlier governance update already
committed. A transaction around this pipeline would change Go's observable
failure state and is therefore rejected.

Set rendering must also propagate its writer error. Independent real `/dev/full`
probes for newly native, nonempty-metadata JSON/table Set completed the write but
returned native exit zero instead of Go exit one. Use a focused checked Set
renderer after persistence, reporting `symbrain memory set: format output: ...`.
Keep committed state when rendering fails. OS-backed stdout errors include Go's
`write /dev/stdout` context; injected writer errors retain the writer's message.
This correction does not claim to resolve inherited Delete/general output error
handling, arbitrary SQLite errors or the broader delegated pipeline.

The permanent gate adds six actual Go/native governance callback pairs (both
formats), validates the exact diagnostic against the ID of the surviving set
audit, compares all committed application state and checks FTS integrity. On a
host exposing `/dev/full`, two more pairs require literal errno diagnostics and
complete committed metadata/staging. A portable failing-writer Rust test runs
on every native platform and verifies exit one plus preservation of a committed
primary row. Unix sink coverage is recorded explicitly; it is not counted as a
Windows runtime result. Native macOS/Windows execution remains required.

Original independent findings, inputs and raw differing outputs are preserved
unchanged under `migration/evidence/memory-cli-758/governed-write-review-28e8/`.
The original reviewed CLI and all five executed test binaries were losslessly
archived and SHA-verified before reusing the released target. The correction
normally integrates parent `7ca3bed0`'s default-path fixture handle ownership fix.
No frozen Go production source, schema-repair ownership or routing admission was
changed to disguise either finding.

Clean corrected source `647477f63688b09f313f93e27cc8222d2f5b1450` passes all
60 Set, 16 Delete, 13 delegated-Go boundary and eight failure-state pairs, with
three actual write controls. All 590 baseline pairs and both baseline controls
pass. Fresh 163 affected Rust tests pass (118 CLI, 33 Memory unit, four Go
fixture, seven store and one complete Activity fixture), with strict Clippy,
format and workflow lint. The source-bound raw receipts, test/binary hashes and
retention checks are under
`migration/evidence/memory-cli-758/governed-write-fix-647477f/verification.json`.
This is author validation and does not replace the required independent review.

The subsequent full independent review closed both original groups and found
actual closed-reader stdout behavior still differed in both Set formats. Its
complete unchanged results and executable archive receipts are retained under
`migration/evidence/memory-cli-758/sigpipe-review-55e3038/`. The scoped process
boundary decision, writer guarantees and remaining limits are documented in
`docs/adr/memory-set-stdout-sigpipe-758.md`.
