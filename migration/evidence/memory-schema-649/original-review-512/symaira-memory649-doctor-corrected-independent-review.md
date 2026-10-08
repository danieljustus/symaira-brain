# Corrected independent full-layer Doctor schema review

Disposition: REQUEST_CHANGES. The original view-substitution P2 is closed. One newly identified Windows acceptance-gate P2 remains. No candidate source edits or GitHub writes.

Immutable source: 51269988152b4118e9df9a97fa6f7fe3730618e3. Exact parent: 0081e1f (full parent hash in receipt). Original reviewed source: 689b1a8bc6a21425fe9285317f0c9b8b785586fd. Worktree /workspace/symaira-memory649-doctor is clean. Native executable SHA256 31782f2f60dcd10399015e43f2790bebe96a122846833c9cfadff4b2cb318aae.

## Original finding closed

The helper now obtains actual sqlite_schema entries with type='table' before accepting columns. A view cannot satisfy an expected table, even when PRAGMA table_info returns all expected names. Actual original SQL reproduction against a Go-created private store now reports memory_db.error="missing required columns: sessions.id, sessions.summary, sessions.updated_at" and human cross with quick_check=ok; the attempted write still proves this is a view. Main database bytes remain unchanged. Fresh supplemental source-pinned Go/native three-observation replay and actual unchanged-Go-as-candidate exit1 false-green control pass. Fresh two schema unit tests and two Doctor schema process tests pass. All eight original additional actual JSON/human cases were independently rerun on this source in separate artifacts, preserving original689 observations.

## P2: new Windows process gate compares incompatible native and Go permission fields

Location: scripts/memory-schema-oracle/replay.py:21, unconditional full healthy memory_db equality. Related existing native branch: rust/symbrain-cli/src/doctor_memory.rs:43-48.

The new three-OS workflow invokes this replay on Windows. Frozen Go cmd/symbrain/cmd_doctor.go:checkMemoryDB uses os.Stat(path).Mode().Perm(), formats those bits and sets mode_ok only for 0600. Pinned Go1.26.7 src/os/types_windows.go:177-182 sets regular-file permissions to 0444 for FILE_ATTRIBUTE_READONLY and otherwise 0666. Python path.chmod(0o600) does not establish POSIX 0600 on Windows; it clears the read-only flag. Consequently the Go-created writable fixture reports mode="0666",mode_ok=false. Existing native non-Unix Doctor unconditionally synthesizes mode="0600",mode_ok=true. The newly added full healthy-field equality therefore fails before the missing-column assertion on native Windows.

This is a source-proven platform mismatch in the new acceptance gate, not a claimed locally executed Windows observation. The mode discrepancy existed in the parent; the new gate now requires contradictory values to match. Fix Windows reporting to the actual Go platform semantics (without claiming POSIX bits establish Windows ACL protection), or explicitly decide/document/assert any deliberate platform divergence. Do not silently strip permission fields from the comparison. A native Windows receipt remains required.

Source proof is retained verbatim and hashed in /tmp/symaira-memory649-corrected-independent-windows-mode-source.txt. Root was notified before this report; this reviewer did not alter the candidate.

## Complete follow-up inspection and evidence

Reviewed the full17-file delta from689: schema helper and unit tests, Doctor CLI regression, Windows runner path normalization, both ADR additions, normal Memory803 cleanup parent scripts and its recorded process/control/native-failure evidence, and all five preserved original review/probe files. Original eleven-file implementation, three-OS workflow, authoritative schema, render/path/error/repair ownership and frozen Go boundary were rechecked against the original review; their unchanged limits still apply.

The Windows owned temporary directory is converted with cygpath -u before trap, git archive/tar extraction and Bash cd; executable paths are subsequently converted to native form where needed. This resolves the previously identified Bash/tar path concern statically. No local Windows runtime or fresh native success is claimed.

The integrated Memory oracle cleanup wraps all three SQLite seed/snapshot connections with closing plus their existing transaction context: commit/rollback occurs before close, so owned files are not removed with retained Python handles. It changes fixture resource ownership, not production Memory behavior or assertions. Its actual source-bound parent receipt retains590/590 CLI pairs and two actual executable controls rejecting32 pairs each, and the original complete Windows failure. The control gzip payload hash was checked against its verification manifest. Its source manifest was compared with current files: integrated Doctor/setup parents and the new Memory export are explicit differences, so590 evidence is not mislabeled as a complete exact512 replay. Fresh native integrated-head Memory acceptance remains required.

All five tracked original689 review/probe files are byte-identical to their original /tmp counterparts. Frozen Go memory/Doctor production and historical fixtures remain unchanged. Doctor continues to use SQLITE_OPEN_READ_ONLY and no Store::open, independent same-binary in-memory expected schema, stable missing-column ordering, existing JSON error/human renderer and exit policy. Missing-name inspection does not assert full type/default/constraint/index/trigger/view-definition equivalence or total WAL sidecar absence. No further product finding was identified within that declared scope.

Inspected Root's exact512 strict Clippy,44 Memory and16 Doctor affected component test results (zero failures/ignored cases) and current process receipt; independently reran the focused tests, three actual Go/native constructor-derived observations, the actual false-green rejection and eight extra process cases. Fresh Mac/Windows/native release gates are not waived.

Separate follow-up artifacts: /tmp/symaira-memory649-corrected-independent-{schema-tests.log,doctor-tests.log,pinned-process.json,pinned-process.log,pinned-process.json.false-green-control.log,extra.py,extra.json,extra.log}. Hash-bound complete receipt /tmp/symaira-memory649-doctor-corrected-independent-review.json. Source and target released with no active compiler or replay.
