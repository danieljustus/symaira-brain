# Independent full-layer review: Doctor schema #649

Disposition: REQUEST_CHANGES. One confirmed P2 finding. No candidate edits or GitHub writes.

Immutable candidate: 689b1a8bc6a21425fe9285317f0c9b8b785586fd.
Exact first-parent base: 83f357f50fe9ee06953a855bd2fbf2efc6068dbc.
Integrated main: 31de72294521701fc4b1a0bce39f03cc34d72e7e; published Memory and Doctor parents are already present.
Worktree: /workspace/symaira-memory649-doctor. Clean before and after this review.
Native executable SHA256: a2684974283ed5eac0e27e5caf53f9b84d27c220b52b60be2a8f312bbe34accd.

## Confirmed finding

[P2] Check required table existence before accepting PRAGMA table_info columns.
Location: rust/symbrain-memory/src/schema_inspection.rs:24 (actual columns are inspected without checking the actual sqlite_schema object type).

On a healthy store created by the real frozen Go CLI, execute:

```sql
DROP TABLE sessions;
CREATE VIEW sessions AS SELECT '' AS id, '' AS summary, '' AS updated_at;
```

Candidate Doctor returns exit 0, memory_db.quick_check="ok", no memory_db.error, and a human green check. This database has no required sessions table; an actual INSERT INTO sessions fails with "cannot modify sessions because it is a view". SQLite PRAGMA table_info reports view columns as well as table columns, so the newly promised required-table presence check is bypassed by a view with the same column names. This is distinct from checking view definitions or every constraint/index, which the ADR deliberately excludes. The matrix and ADR explicitly promise required table/column presence. Check actual sqlite_schema.type='table' for each expected table before considering columns present, and retain a same-column view regression.

Actual executable reproduction and unchanged main database hashes: /tmp/symaira-memory649-independent-extra.py, .json, .log. The JSON records both full memory_db output and human line plus the actual SQLite write failure. No operator state was used.

## Inspection completed

Read AGENTS.md and every changed file in the exact first-parent diff (11 files, 550 insertions/68 deletions):
- .github/workflows/memory-schema-native.yml
- docs/adr/649-doctor-schema-inspection.md
- migration/contract-matrix.csv
- migration/implementation-plan.md
- rust/symbrain-cli/src/doctor_core.rs
- rust/symbrain-cli/src/doctor_memory.rs
- rust/symbrain-cli/tests/doctor_memory_schema.rs
- rust/symbrain-memory/src/lib.rs
- rust/symbrain-memory/src/schema_inspection.rs
- scripts/memory-schema-oracle/replay.py
- scripts/memory-schema-oracle/run.sh

Also inspected the existing schema.rs, migration.rs, store.rs, store_contract_tests.rs, Doctor CLI/check/render/types/path selection, Cargo manifests, frozen Go Doctor check/render and applicable immutable memory sources.

The extraction preserves existing missing-path/stat/open/quick_check errors and permission/path/legacy fields. The new production Doctor path opens the existing file with SQLITE_OPEN_READ_ONLY and never invokes Store::open or actual-database migrations. Expected facts are created only in an independent in-memory connection using the same binary's SCHEMA; identifiers are escaped and sourced only from that schema. Diagnostic ordering is stable. Existing human error rendering and report exit policy are preserved. Missing table columns, an individual missing column, empty DB, corrupt DB and directory-instead-of-file were also exercised through the actual native executable with JSON/human output and unchanged main database content.

The original five-column issue is reproduced despite applied migration bookkeeping and quick_check=ok. The public Store integration test retains a Unicode memory row, migration count and repeated-open idempotence. Existing atomic repair/concurrency/rollback logic remains separate and unchanged. Frozen Go memory/Doctor sources and historical memory fixtures were not modified by the candidate.

An uppercase-only column rename was explored and produced a missing canonical-name diagnostic. It is not a separate new finding here: the already existing repair path also treats canonical column names exactly, and this increment does not promise general SQL identifier equivalence. Required-column presence does not prove type/default/constraint/index/trigger/view-definition equivalence; the ADR states this limit. Read-only SQLite may use WAL sidecars; the tests prove unchanged main database contents, not total sidecar absence.

## Independent fresh verification

- Focused schema_inspection unit tests: 2 passed, 0 failed.
- Doctor schema actual CLI integration: 1 passed, 0 failed.
- Fresh source-pinned scripts/memory-schema-oracle/run.sh: archived immutable Go dcddcef0df5789123c7c9a7ebe6e01f10e941f2c, Go 1.26.7/corekit v0.17.0, actual constructor then 3 Go/native observations passed (healthy, broken five-column store, native-repaired store). Full healthy memory_db fields match; broken-store difference is intentionally asserted, not hidden by projection. Native Go fallback is absent in the private environment. Actual unchanged-Go-as-candidate false-green control exited 1 for the required missing-column assertion; its original traceback is retained.
- Eight additional owned process cases exercised both human and JSON output. Main database content unchanged in every file case. View substitute reproduces the finding.
- Inspected Root's clean-head strict Clippy and component logs: Memory 44 tests passed; Doctor component integration 15 passed; no failures/ignored cases. These checks are Root-authored evidence, not misrepresented as repeated independent full suites.

Independent receipts/logs: /tmp/symaira-memory649-independent-{schema-tests.log,doctor-tests.log,pinned-process.json,pinned-process.log,pinned-process.json.false-green-control.log,extra.py,extra.json,extra.log}. /tmp/symaira-memory649-doctor-independent-review.json binds their bytes and changed source bytes.

## Native gates and remaining acceptance

The dedicated workflow covers real Linux/macOS/Windows, strict native component build/Clippy/fmt, affected Memory/Doctor tests, actual source-pinned Go/native process replay and actual false-green rejection; receipts are retained even on failure. Matrix DB-005 honestly remains pending. No fresh macOS/Windows execution is claimed by this Linux review, and this review does not waive native jobs, eventual integrated-head checks or the shipped #649 acceptance requirements.

One additional static Windows workflow concern was reported by Root: run.sh passes a native RUNNER_TEMP-derived backslash path directly to tar -C before MSYS path normalization. Root has an analogous actual Guard Windows failure, but this reviewer did not reproduce that path on Windows; it is recorded as an unresolved native-gate concern, not an independently executed second product finding. Normalize the owned path for Bash/tar and verify the real native Windows job.
