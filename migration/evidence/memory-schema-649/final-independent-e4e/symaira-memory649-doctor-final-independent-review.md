# Final independent full-layer Doctor schema review #649

Disposition: APPROVE the immutable source after independent closure of both findings. Fresh native Linux/macOS/Windows exact-head acceptance remains required before merge; no native gate or shipped #649 requirement is waived.

Immutable head: e4e06371b6f90dedb920072f36bdfa7231200dab.
Exact parent: 8ddeb486c86e2c56eb78b9408443a136235164f5.
Original reviewed base: 83f357f50fe9ee06953a855bd2fbf2efc6068dbc.
Original rejected sources: 689b1a8bc6a21425fe9285317f0c9b8b785586fd and 51269988152b4118e9df9a97fa6f7fe3730618e3.
Clean worktree: /workspace/symaira-memory649-doctor.
Native executable SHA256: 781694a81155e5b01e19b53c3ffaae6c319c7ce8be1287fffab9a41798ede6cb.
No candidate edits or GitHub writes by this reviewer.

## Finding closures

The original actual view-substitution false green is closed. Actual sqlite_schema.type='table' presence is checked before accepting column names. Rerunning the same SQL against a healthy real-Go-created private store gives quick_check=ok plus the required missing sessions columns in memory_db.error and human cross. The attempted insert still proves the view cannot satisfy the table contract. Main database bytes remain unchanged. Required columns come from the same binary's authoritative schema in a separate in-memory connection; migration bookkeeping is not accepted as DDL proof.

The second review's source-proven Windows healthy-field gate mismatch is closed in source. Pinned Go 1.26.7 maps FILE_ATTRIBUTE_READONLY to0444, otherwise0666, adding0111 for directory metadata. Native reporting now derives exactly these facts from metadata.permissions().readonly() and metadata.is_dir(), and mode_ok remainsfalse. This does not claim POSIX0600 protection or private Windows ACLs. The new actual native Windows oracle fixture compares a readonly file before restoring attributes for subsequent surgery; full memory_db fields still compare, without permission-field projection. A Windows-only CLI test exercises writable/readonly directories at default.db, expects0777/0555 and an open error, and restores attributes for cleanup. Those Windows-only paths were statically reviewed against the pinned SDK; this Linux review does not claim to have executed them.

The owned temporary path is normalized through cygpath -u before Bash/tar use and converted to native executable paths where needed. Static Windows Bash/tar portability concern is closed; real Windows execution remains mandatory.

## Complete layers reviewed

Read AGENTS.md and the complete original eleven-file Doctor increment: production helper/extraction/Memory export, focused unit and actual CLI tests, three-OS workflow, source-pinned Go/native runner/replay, public ADR, matrix DB-005 and implementation plan. Rechecked all these layers on the final source. Also reviewed the complete seventeen-file correction/integrated-parent delta from689 and seven-file Windows follow-up delta from512: actual table-type guard, view regressions, Windows stat facts and directory/read-only fixtures, runner path conversion, both ADR additions, integrated Memory oracle connection cleanup and all retained evidence.

Existing schema/migration/Store, Doctor CLI/check/render/types/path logic, manifests and frozen Go memory/Doctor behavior were inspected in the original and corrected reviews. Store repair remains atomic/idempotent and separate. Production Doctor opens only the existing file with SQLITE_OPEN_READ_ONLY, never Store::open and never actual-store migrations. Missing/open/stat/integrity error behavior, existing report exit policy, JSON fields and human renderer stay intact. Identifier escaping uses only binary-owned expected names; missing diagnostics are sorted. New focused production/test modules remain below400 lines.

The normal Memory803 cleanup parent wraps all three Python SQLite connections with closing plus their transaction context, preserving commit/rollback before close and all comparisons. Its retained590/590 CLI parent pairs and two actual executable controls (32 rejected pairs each) were inspected and their source/control payload hashes verified during the corrected review. The source-map differences from that parent are explicitly integrated Doctor/setup work plus the Memory export; no obsolete parent receipt is represented as an exact-final-head full CLI replay. Fresh integrated native acceptance remains required.

All eight tracked original689/512 review/receipt/probe/SDK-excerpt artifacts are byte-identical to their original /tmp versions. Frozen Go Doctor/memory sources and historical fixtures remain unchanged. Original failed native cleanup evidence stays retained. No report projection hides the intentional missing-column correction or the platform permission facts.

## Fresh independent execution on exact final source

- Two focused schema inspection unit tests passed; zero failures.
- Two actual Doctor schema CLI integration tests passed, including same-column view and missing-five-column cases; zero failures.
- The source-pinned runner archived immutable Go dcddcef0df5789123c7c9a7ebe6e01f10e941f2c and built with Go1.26.7/corekitv0.17.0. Three actual Go/native constructor-derived observations passed: healthy, applied-migrations/five-missing-columns, native repaired. Healthy full memory_db fields match; the broken diagnostic difference is explicitly asserted. Main database hashes remain unchanged by diagnosis.
- The real unchanged Go executable supplied as purported corrected candidate exited1 at the intended missing-column assertion. Original traceback retained. This is an actual executable control, not an invented comparator mutation.
- Eight original additional private cases rerun with both JSON and human output: healthy, missing table, single missing column, empty file, corrupt file, directory, same-column view and canonical-name case rename. Main database content remains unchanged. The view reproducer confirms finding closure; existing canonical-name limits remain documented.
- Inspected Root's exact-final-source strict Clippy/format/build and full affected component logs:44 Memory and16 Doctor tests passed, zero failures/ignored cases. These are Root-authored results, distinguished from the independently repeated focused cases. The Windows-only additional directory test is excluded on Linux and must run natively.

Fresh independent paths: /tmp/symaira-memory649-final-independent-{schema-tests.log,doctor-tests.log,pinned-process.json,pinned-process.log,pinned-process.json.false-green-control.log,extra.py,extra.json,extra.log}. Exact head, source/evidence hashes and original finding closures are bound in /tmp/symaira-memory649-doctor-final-independent-review.json.

The declared diagnostic proves required canonical table/column presence, not full type/default/constraint/index/trigger/view-definition equivalence. Read-only SQLite can use WAL sidecars; no main database content modification is proven, rather than absolute sidecar absence. Fresh native Windows readonly behavior and all three OS jobs remain an acceptance gate. No further finding within this increment was identified. Candidate and target released clean/idle; no compiler or replay remains active.
