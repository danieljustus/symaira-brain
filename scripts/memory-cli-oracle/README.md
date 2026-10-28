# Native memory CLI process contracts (#758)

Run `cargo build -p symbrain-cli --locked`, then
`python3 scripts/memory-cli-oracle/replay.py --report /tmp/memory-cli-process.json`.
The harness archives and builds immutable Go `dcddcef0` with pinned CoreKit
v0.17.0 configkit for non-sync families. Only `sync_preflight` and `sync_relay`
(including every sync-pull case) use `fc5252d2e88c0ac2f8fe945113f9494084916558`
(main's #980 squash commit) with independently hash-verified CoreKit v0.18.2
configkit: this revision contains #970's fix for #854 and #980's chronological
`symbrain_sync_time` cursor comparisons. Its `cmd/` and `go.mod`/`go.sum` bytes are
identical to the former pin `56ef5353`; only `internal/memory` changed, so the
move is exactly the #980 behavior. The native store registers the same SQL
function (`rust/symbrain-memory/src/sync_time.rs`) and
`python3 scripts/memory-cli-oracle/sync_time_fixture.py --check` proves its
normalization against vectors recorded from the Go function at that revision. No fixtures are rewritten. Both builds explicitly use
`GOTOOLCHAIN=go1.26.7`, matching `memory-cli-native.yml` rather than ambient Go.
The report retains both executable, immutable source, SDK and candidate source
hash sets in `oracle_provenance`; each record names its `go_oracle_revision`.
The top-level legacy Go fields describe the first selected oracle, not every
family. `--go` permits reuse of one explicitly supplied immutable local oracle
for a single revision's families; its SHA is recorded. It is refused with `all`
because one executable cannot bind two revisions. `CARGO_TARGET_DIR` is honored
when finding the default Rust binary.
`--family arguments|configuration|seeded_reads|corrupt_reads` supports focused diagnosis.

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

Linux/macOS execute 655 cases; Windows executes 584 because 37 argv byte cases
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
The Go decoder diagnostic must match the independently specified expected bytes;
an unrelated transport/database error cannot satisfy a corrupt-row case. No-Go
observations require the owned absent-binary diagnostic with OS error 2, empty
stdout and exit 1, rather than merely any failure. Focused unit controls reject
generic error substitution and wrong failure causes without counting those units
as additional real process cases.

The CLI's read-only stored-JSON admission is conservative across the entire
relevant table. An unsupported unselected row can retain Go for the whole
command; missing historical columns/tables still reach native transactional
migration. Shared native row decoders now propagate JSON failures instead of
returning empty success. Exact Go diagnostics for unsupported JSON remain on Go.
Admission and native hydration use separate connections. Concurrent external
rewrites can invalidate admission before hydration; malformed JSON still errors,
but exact Go diagnostics and null/type semantics are not an atomic concurrent
mutation guarantee in this slice.

`--family store_creation` (`creation_cases.py`, 48 Unix scenarios, 51 on macOS where the root-owned `/tmp` link is exercised, fewer on Windows) replays missing-store creation and every directory/open failure shape for list, rules, query-log, set and delete, plus `--db /dev/null` and a FIFO for all six verbs (Go never blocks on them; a CLI timeout is a mismatch). The SQLite code printed for these is environment-dependent and is never hard-coded: `/dev/null` is `unable to open database file (14)` on the local macOS 27 host but `attempt to write a readonly database (1544)` on Linux as a non-root user (the extended code of the WAL pragma, which the shipped driver prints because it enables extended result codes after `sqlite3_open_v2`), a FIFO is `disk I/O error (10)` (primary code: the failure is inside the open call); only the live Go comparison asserts the exact bytes per OS, `memory_store_creation.rs` pins the `<text> (<code>)` shape and compares stdout, stderr, exit and the whole created tree with permission bits (directories `0700`, database `0600` under umask 022). `creation_controls.py` runs three real-process mutants (directory mode, stderr byte, exit status) that the family must reject through exactly their own field.

`--family sync_preflight` (`sync_cases.py`, 192 cases) covers the `memory sync` argument checks that precede database and network work; `sync_controls.py` runs three real-process mutants. `--family sync_relay` (`sync_relay_cases.py`, `sync_pull_cases.py`, `sync_relay_server.py`, 590 scenarios) runs the immutable Go CLI and the native CLI against their own loopback fake relay (bytes of every response are scripted; nothing leaves 127.0.0.1) and compares stdout, stderr and exit of every step, every request the relay received (method, target, all headers) and the full database state (audit-log ids and wall-clock stamps are normalised only where they have the shipped spelling). 424 native scenarios must match with the Go fallback absent - empty exchanges, and since slice 3 relay replies with memories, tombstones, pagination and server-time handling; 166 hand-off scenarios must stop at the owned absent-binary diagnostic, leave every local table (memories, audit, oplog, cursors, entities) exactly as seeded, and have sent no request or only read-only GETs that Go sent too, in Go's order. `SYNC_RELAY_ONLY=<regex>` restricts the family to matching scenario names (used by the source-mutant controls). `sync_relay_controls.py` (Unix only) runs five real-process mutants (stdout, stderr, exit, an extra request, an extra database row) that must each be rejected through exactly their own observation key. `sync_pull_controls.py` (Unix only) rebuilds the CLI from a scratch copy of the tree with nine source mutants of the pull path (equal-timestamp overwrite, tombstone and delete comparison, only the first page, the pagination cursor never sent, cursor ignoring entry times, no audit row, no push probe, a failing statement skipped instead of rolled back; `--cargo` takes a cargo command prefix) and requires the scenarios exercising each to reject it, with no rejection the mutation cannot explain. On Windows the connection-refused wording scenarios are skipped with a printed notice and the created-tree mode comparison is not made.

## Relay mismatch diagnosis (#958)

On macOS 27.2, an unmodified `7f5b98c1a` Rust binary against the original
`dcddcef0` Go oracle reproduces the seven failures with ambient Go 1.27.3:
`body-12`, `decode-failure-14`, `-15`, `-19`, `-20`, `-21`, `-26`.
The changed bytes are only JSON syntax-error diagnostics: Go 1.27 decodes
UTF-8 runes (BOM and multibyte characters) instead of quoting the first byte,
and quotes Latin-1 nonprintable characters with `\\xNN` instead of `\\u00NN`.
The same eight-case probe (these seven plus
`pull-same-run-twice-is-idempotent`) passes 1/8 with Go 1.27.3 and 8/8 with
CI's Go 1.26.7. This is toolchain drift, not a macOS-specific defect and not
caused by the Rust tombstone fix `b23d25805`. The oracle now pins its build
and reflected-schema toolchain explicitly; no comparison is weakened.

`pull-same-run-twice-is-idempotent` is separate: the old Rust binary matches
the old Go oracle but lacks the absent-row `c` tombstone when compared with
fixed Go. The retained Rust tombstone regression fix makes it match fixed Go;
it is part of #854, not one of the seven diagnostic failures.
`delete-missing-id` now uses ordinary strict equality, with no known-defect
marker, expected-state rewrite or special-case comparison. Against fixed Go,
the repaired candidate passes all 590 relay scenarios and all 1454 process
cases (including 192 sync-preflight cases) on this host. These are local
macOS results, not new Linux/Windows CI receipts.

This increment proves parser/config and seeded read behavior. It does not certify
governed writes, corrupt database error strings, configured Hamming prefilter, evidence JSONL decoding or MCP
cutover. Those remain open #758 acceptance work. No Go source or frozen fixture
is modified. Native Ubuntu/macOS/Windows workflow receipts are required; local
Linux execution alone does not satisfy the release gate.
