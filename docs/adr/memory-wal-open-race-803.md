# Bound WAL setup contention without replaying migrations

Status: focused Linux author checks passed; mixed-lock control and independent/native3 pending.
Refs #649/#758 and PR803/PR811. Base: immutable PR803 `6ae73a7`.

Windows job `111341331340` in run `37170162739` reports three worker failures
inside the unchanged public concurrent-open test: Store::open returns SQLite
primary/extended BUSY 5. The test retains ten rounds of eight openers and every
data/write/assertion. The complete CRLF raw log, decoded record and source
bindings are preserved in `migration/evidence/memory-open-race-803`.
PR803's green Windows run does not invalidate this actual intermittent failure.

The outer unwrap location does not identify the failed initialization phase.
The bundled SQLite source does establish a relevant lock path: OP_JournalMode
may call sqlite3BtreeSetVersion before the migration's IMMEDIATE transaction;
that operation first reads and then requests an exclusive rollback-journal
transaction. Pager shared-to-reserved/exclusive upgrades can return BUSY without
invoking the busy handler. WAL header discovery and last-close/checkpoint
contention add further opening boundaries. These are mechanisms, not proof that
the Windows job failed at one particular internal call.

Prepare a narrow WAL-only retry with one monotonic five-second budget. Each
attempt receives only that budget's remaining SQLite busy timeout; an early
plain BUSY 5 may retry after the statement has released its implicit transaction.
Require Connection::is_autocommit. A small bounded sleep yields to the current
owner rather than spinning. Restore the usual five-second connection timeout
before the unchanged BEGIN IMMEDIATE and schema/data/index/ledger transaction.
The retry does not extend the WAL budget or replay any migration statement.

Retry only the observed typed primary/extended BUSY 5. SQLITE_BUSY_SNAPSHOT,
BUSY_RECOVERY, BUSY_TIMEOUT, SQLITE_LOCKED, I/O, permission, corruption and other
errors propagate unchanged; no message parsing or global error reclassification.
Never retry inside an active caller transaction. Do not introduce a process-wide
mutex, filename-lock registry, larger timeout, persistent marker or blanket
constructor replay. Those approaches hide ownership/aliasing or actual timeout
failures and do not establish cross-process correctness.

Keep foreign_keys before WAL and secure_delete after it, with pragmas outside
IMMEDIATE. In-memory journal behavior stays SQLite's existing memory mode.
The helper changes no application schema/data and retains original SQL errors.

The first actual Linux control at source `62b2757` disproved the retained-reader
assumption: its busy callback ran once. All original tests and the held-reader
five-second budget passed (37 passed/one new control failed). Preserve the full
failed log/source map and gzip-roundtripped executed test binary. SQLite's
reader conflict permits RESERVED acquisition, then waits on EXCLUSIVE, where
the handler runs. Correct the fixture to a reserved writer: that blocks the
SHARED-to-RESERVED upgrade, where the handler is skipped. This changes the new
lock-owner fixture, not the production budget, helper or original assertions.

Allocation-stage proof must first bind the actual failing phase. Prepare a real
rollback-mode reserved-writer control whose WAL transition returns plain BUSY while the
busy callback is not invoked, then release the writer after observing the
helper's real BUSY attempt and require WAL setup success. A retained reader
must exhaust the same five-second budget and preserve the original error/state.
Also retain a reserved writer through expiry, require multiple genuine early
BUSY attempts, and bind elapsed time/error/restored timeout/unchanged rows to the
one shared budget. Test error classification and active-transaction refusal separately. Keep the
original ten-by-eight concurrency assertions verbatim; exercise opening and
closing in the full inherited gates and native Windows/macOS/Linux CI.

At source `bdf2371`, 39 library tests, strict Memory all-target/all-feature
Clippy, formatting and actionlint passed. The held writer caused 984 real early
BUSY attempts in 5.000088261 seconds; the held reader waited 5.007606427 seconds.
The original 80 concurrent public opens/writes passed without assertion changes.
All current ELF bytes are gzip-roundtrip/SHA archived, with the one actually
executed Memory test binary separately identified. The first actionlint launch
failed because PATH lacked the tool; its literal log is retained and the
existing absolute tool invocation passed. These single-phase controls still
need a mixed writer-to-reader control: initial skipped-handler time must reduce
the later genuine SQLite-handler wait, rather than granting it five new seconds.

This proposal does not assert that the Windows race is closed. If actual phase
proof identifies BEGIN IMMEDIATE or migration/commit contention instead, retain
that evidence and revise the candidate without lengthening the timeout. Only a
validated and independently reviewed correction may be integrated normally into
the unchanged historical repair successor `dba88d2`. No #649/#758 closure follows
from this static preparation.
