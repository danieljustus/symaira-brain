# Codex resource timestamps keep the Go parser domain (#761)

Status: source implementation prepared; no compiler, SDK behavior or product
runtime has been allocated. User-delegated product authority permits this
local correctness repair. No routing, registry or provider admission changes.

Different-author review of immutable `e55ea752b28d88df0b9d2248a8d32cb6d971f9e5`
requested one additional inherited P2 after approving the three previous local
corrections at source level. Original report SHA
`193abec9ab1e8089af2e02dbd5c65ed98c8eddf96f40ecde4d7cac40aa6a9841`
and receipt SHA
`570bdd36dc16e29d175e4b14d9104a631a8a93ffd880593df46d1c2c1f83b1e5`
remain byte-exact, together with all 82 proof records and the receipt. Their
lossless retention precedes this isolated successor; the original e55 and 2946
checkpoints remain unchanged.

Frozen Codex Memory parses the first 19 filename bytes with
`time.Parse("2006-01-02T15-04-05", ...)`. SDK 1.26.7 rejects second60. Locked
Chrono0.4.45 accepts it as second59 plus 1,000,000,000 nanoseconds. Consequently
`2026-01-01T23-59-60-owned-6h-context.md` previously acquired native timed
metadata and could cover a following ten-minute resource. Go treats the file
as consolidated Markdown. These conclusions are source-derived; no before-fix
Go/native execution is represented as observed.

The existing local `markdown::resource` owner now rejects a parsed nanosecond
value >=1e9. Both discovery's parent collection and direct import already use
that owner, so the correction consistently retains the file as consolidated
Markdown and excludes it from timed coverage. Valid second59 and ordinary
six-hour/ten-minute resources keep their current behavior. The document is not
dropped. No shared timestamp formatter, DTO, retention owner, path conversion
or other consumer changes. The previous three corrections, all original
93 constructor /12 retention recipes, 32 unit definitions and six real-process
controls remain byte-exact.

Three additional private parser tests are prepared. Six public-constructor
cases cover invalid6h/10min, valid59 positive controls, invalid-parent/valid-child
and valid-parent coverage. A separate prepared gate appends those six to the
unchanged93 cases, compares full99 ordered actual Go/native reports and complete
source snapshots, and preserves the original93 input bytes. No expected result
normalization or fallback is introduced. Direct imports of discovered invalid
Markdown remain included by both existing public-owner callers.

One additional actual-process mutation recipe changes only an owned invalid6h
filename to its valid59 counterpart, captures the mutant's readonly snapshot,
requires the observed consolidated→6h metadata change and exactly one changed
case, then restores the filename and parent mtime. The literal full comparator
must reject that mutant while every bootstrap succeeds. This recipe is prepared,
not executed; the previous six controls are still mandatory.

The complete index retains every parent path; only this parser/module and
three ledger rows change from e55. The sparse materialization stays below15MiB.
No new dependency, target, cache, SDK/product execution, port or GitHub operation
is performed. Full different-author source review, actual99 constructor/12
retention/35unit definitions, original and additive controls, affected parent
strict checks, typed I/O/time domains, Windows path gates and native-three-OS
checks remain required before acceptance. Ten other importer families and
registry/engine governance remain open; this does not close #761.
