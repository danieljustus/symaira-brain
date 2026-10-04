# Brain Memory startup: six source-review corrections

Status: prepared source successor of `9804c0b1b278e71ce5d533e8ee348945b68e530e`.
No compiler, SDK program, product, SQLite fixture, network or port was executed
for this checkpoint. Native Linux, Windows and macOS and a different-author
full review remain required. Issues765/759 and the shared Store cutover stay open.

The complete different-author bc0 review, receipt and57 new proof files were
losslessly retained before this isolated sparse worktree was created. The60
records are under `migration/evidence/brain-config13/startup-review-corrections/
original-review`; `retention.json` preserves original paths and SHA256 identities.
The original bc0/9804 sources, original102/152/182/146 inputs/reports,41 startup
plans, four existing state controls and two prepared Gateway owner tests are
unchanged. This successor adds criteria rather than recasting prior failures.

## Decisions and source boundaries

1. Persisted rotation expiry has a private `time.Time` owner. It reads original
   quoted JSON bytes, without JSON-unescaping, and mirrors pinned Go1.26.7's
   deliberately disabled strict RFC3339 fallback. One-digit hours, comma
   fractions and offsets24/60 can parse; lowercase separators, leap seconds
   and JSON-escaped dates cannot. Parsing and later marshaling remain separate:
   an accepted24-hour offset must still fail marshal. This does not change
   general Store timestamps or Core's existing time helpers.
2. A private iterative JSON scanner validates the entire original plaintext
   before typed records. It preserves duplicate-field order, raw time spans,
   syntax/depth errors and Go's saved ordinary type error versus immediately
   fatal `Time.UnmarshalJSON` error. Unknown numeric values are grammar-only,
   including float overflow. Authenticated malformed plaintext gets the Go
   decoding warning; failed legacy plaintext parsing still reports the original
   AES decryption error. Keys and plaintext secret values never enter warnings.
3. JWT/rotation mkdir follows Go's lexical recursion and reports the blocking
   component, preserving raw bytes. Database SafeMkdirAll remains a separate
   permission/symlink owner. Its Windows non-directory message uses CoreKit's
   `path is not a regular file`. Only the existing Core lexical volume length
   helper is additively exported; no global path codec behavior changes.
4. Store configuration exposes private typed connection, secure-delete and
   migration phases. The public Store error API is unchanged. The existing
   IMMEDIATE transaction, schema/parity/index order, bookkeeping and rollback
   remain intact. The startup formatter no longer labels every error as opening
   a connection. Exact modernc/rusqlite diagnostic detail and Go migration-file
   context remain actual paired-runtime gates: the merged native schema cannot
   honestly invent an old Go migration filename. The new incompatible-schema
   process case retains and compares full diagnostics and state, so any remaining
   difference will fail rather than be normalized or waived.
5. The existing bounded provider capture is extracted into one focused owner
   and retains `ExitStatus`. The additive raw Memory reference API formats the
   actual Go signal name, core bit and Windows unsigned large exit hex, retaining
   stderr bytes and trim order. Existing String Usage callers retain their prior
   diagnostics. Linux/Darwin signal names are source-derived from pinned SDK
   tables; this is not actual signal or native Windows evidence. The inherited
   non-success descendant-cleanup limitation stays explicit and runtime-gated.
6. Startup state acceptance keeps trigger/view identity, table ownership and
   SQL token bodies. Only this schema-program representation ignores comments,
   formatting and unquoted SQL token case. Quoted tokens remain exact; raw DDL,
   rows/defaults/indexes, file bytes and diagnostic bytes are never normalized.
   Removed or changed programs must reject. Owned cloned-engine semantic checks
   continue and real missing/changed-trigger/added/changed-view controls are
   additive to the four original state mutants.

## Prepared acceptance

The additive runner has81 Unix plans: the original41 plus40 new authenticated
JSON/time, nested raw blocker, incompatible schema and provider-signal cases.
The pure Windows branch projection has69 plans (original36 plus33), including large exit codes; it is not counted
as Unix runtime. The original41 input dictionaries and bytes are checked as
an exact prefix. A separate deterministic AES fixture producer encrypts arbitrary
synthetic plaintext with fixed fixture salt/nonce. Production key/salt/nonce
generation remains CSPRNG-owned. Readonly authenticated fixtures are not counted
as fresh entropy samples; only owner-created envelopes require non-repetition.
Every constructed fixture input/envelope, provider script, request, warning,
state and observer role is raw-byte retained with hashes.

The prepared runtime driver runs full workspace/all-target/all-feature tests,
strict Clippy, format/actionlint, actual frozen Go CLI and constructor, original
41 and additive81 startup processes, all eight state controls, original
102/152/182/146 families and their actual controls, historical helpers, inherited
Setup/Doctor/Guard/Memory suites and doc tests on one explicitly allocated target.
It retains every failed stage and archives actual binaries before correction.
700MiB floors cover target, output, private Go cache and TMPDIR; new outputs stay
under `/workspace`. No present checkpoint claims these prepared gates passed.

The raw-key/fallback cases use authenticated readonly envelopes to observe the
retained key, in addition to the full warning/request/state comparison. Cloned
engine checks exercise all three sync-oplog operations and sync_exclude entry,
continued exclusion, deletion and exit from exclusion.

Five executed pure Python controls reject removed/changed trigger/view bodies
and accept formatting-only changes, without SQLite, filesystem fixtures or
product execution. This limited source-tool observation is retained separately
in `pure-preparation.json`; it is not compiled acceptance or issue closure.
