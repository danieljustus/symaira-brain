# Native memory CLI process contracts (#758)

Run `cargo build -p symbrain-cli --locked`, then
`python3 scripts/memory-cli-oracle/replay.py --report /tmp/memory-cli-process.json`.
The harness archives and builds immutable Go `dcddcef0`, verifies pinned CoreKit
v0.17.0 configkit, and records both executable hashes and candidate source hashes.
`--go` permits reuse of an explicitly supplied immutable local oracle; its SHA
is recorded. `--family arguments|configuration|seeded_reads|corrupt_reads` supports focused diagnosis.

Every case uses a disposable HOME, USERPROFILE, cwd, and XDG directories. Runtime
PATH is empty and the explicit Go fallback path is absent, except for the
explicitly labeled unsupported stored-JSON cases described below. The argument family
compares literal stdout/stderr bytes and exits for help, unknown commands/flags,
flag reordering/termination, repeated aliases, Go base-zero integers, boolean
spelling/errors and Unix invalid UTF-8. The config family seeds four independent
database choices with distinct stable rows and checks all 86 known typed fields,
zero-value merge semantics, whole-config reset on errors, project/env precedence,
unknown keys, duplicate TOML and float syntax/range. Every seeded database column,
row and blob remains strictly identical before/after each read in both processes.
An additional 32 read cases seed 1,100 memories and 80 query-log rows to prove
default/clamped limits, repeated scope/actor aliases, rules and summary output.
These cover both successful table and JSON rendering; success exits are required.
Three configured searches compare real Go/Rust request paths and model/input
payloads against an owned loopback server, covering global/project/env overrides.
Rules fixtures populate the separate rules table, metadata and actor fields with
HTML and JavaScript-separator characters; successful output must contain rule IDs.
Seven `database_paths` cases compare absolute/relative/unset/empty XDG roots,
shared current/legacy roots, current precedence and directory-versus-file type.
Reflection executed against Go verifies that the Rust descriptor covers exactly
the immutable 86-field typed schema, including `bm25_weight`.

Linux/macOS execute 621 cases; Windows executes 584 because 37 argv byte cases
require Unix. `controls.py` compiles a disposable process wrapper that runs the
actual Rust candidate and then changes one semantic stdout token or its success
exit code. Both real replays must fail for the intended comparison assertion,
while every seeded database remains unchanged. This catches accidental omission
of output or exit checks without a formatter-induced mismatch.

The 31 `corrupt_reads` cases cover malformed/empty and wrong-shaped metadata,
duplicate-key type errors, Go null-map semantics, malformed/overflow vectors and
null vector elements. These are generated fault-injection fixtures, not captured
damaged installations. Exact `json_extract` oplog triggers are temporarily removed
only while injecting the disposable rows, then restored before either CLI runs.
Every case compares exact output/exit and full table state; trigger definitions
also remain exact. Search uses a real owned loopback embedding server and no
inherited provider credentials. The 29 unsupported cases explicitly allow the
immutable Go binary for error/representation parity and additionally require
failure with Go absent; the two valid string-map controls require native success
with Go absent. These fallback cases are not evidence of Go-independent reads.

The CLI's read-only stored-JSON admission is conservative across the entire
relevant table. An unsupported unselected row can retain Go for the whole
command; missing historical columns/tables still reach native transactional
migration. Shared native row decoders now propagate JSON failures instead of
returning empty success. Exact Go diagnostics for unsupported JSON remain on Go.

This increment proves parser/config and seeded read behavior. It does not certify
governed writes, missing/corrupt/unwritable database error strings, database file
creation modes, configured Hamming prefilter, evidence JSONL decoding or MCP
cutover. Those remain open #758 acceptance work. No Go source or frozen fixture
is modified. Native Ubuntu/macOS/Windows workflow receipts are required; local
Linux execution alone does not satisfy the release gate.
