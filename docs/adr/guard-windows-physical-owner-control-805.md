# Preserve a genuine physical-owner mutation on Windows

The original native Windows run37186919053/job111390702060 remains a failed
acceptance gate. Publicationa1a29878 and clean CI merge375d77c5 are distinct
identities. The complete BOM/CRLF log, API responses, original artifactZIP and
every member, including actual native and Go PE bytes, are retained before
changing the harness. Their source and executable hashes match the publication
and frozen Go oracle. Original production and comparison behavior is unchanged.

The failure was the `incidental mutant failure` assertion in config_paths.py.
The control required the mutated native process to return0 and warn about
`physical_owner`, before requiring the real Go/native comparator to reject it.
It did not accept an unrelated crashing mutant. Unfortunately the original
runner wrote its report only after this assertion; the failed control's actual
stdout/stderr, environment and preceding config-path pairs were not uploaded.
The original artifact has eight members and no config-path report. Preserve
that provenance gap; no original child output or exact causal attribution is
invented from the assertion alone.

The source nevertheless contains a concrete Windows fixture defect. The
original mutation calls realpath on `owned-root/link/../cfg`. CPython's Windows
realpath applies normpath before resolving links, removing `link/..` first.
The new spelling can therefore still select the original lexical semantic
owner, instead of the intended healthy physical owner. This is not a reliable
physical-owner mutant on Windows. A source-only observation of that defect is
not proof of the original child error, whose bytes are missing.

Keep the original Unix mutation unchanged. On Windows, resolve only the owned
junction prefix first, then join its parent and the original tail. That keeps
the same intentionally wrong physical config owner across path-library
normalization. Verify the distinct physical config sentinel before running the
native process. Retain both strict incidental-failure checks, the original
semantic case and the unmodified full Go/native/state comparator; the control
must still prove an actual successful wrong-owner process before rejecting it.

Retain normal and control observations before assertions in the next actual
run, with raw stdout/stderr, actual exits, argv/cwd, owned environment, original
config bytes and read-only metadata. The generated wrapper also retains its
inner native child output, actual exit and selected environment in an owned
sidecar outside the read-only fixture, before its strict assertion. The outer
journal records that sidecar even when the wrapper fails. Add the receipt and progress
journal to the existing artifact upload. This supplies new current evidence;
it cannot reconstruct omitted original observations. Keep normal case vectors,
timeouts, warning bytes, gating rules, Go/native production, frozen sources and
all other controls unchanged. A failed or incomplete next run remains a failure.

This successor is source-only. Portable path-library/progress controls and AST
equivalence checks do not run the Go/native product or Windows APIs. Actual
native Windows full config-path pairs plus the genuine owner mutant, all
existing Guard gates and three-OS CI remain required. No scope closure, new
fallback, product waiver or self-approval is granted.

The first authored portable source checkpointd42be7c3 had eight tests with two
passes, two failures and four errors. Its complete original outputs and source
hashes remain retained. They exposed a misspelled NamedTemporaryFile API and
test-only expectations omitting CPython's second prefix-validation lookup.
Correct those concrete issues, keep the original strict control criteria and
record the complete two-request sequences. Ten final portable tests also
exercise the actual observer's partial-timeout and read-only-failure journals
with mocked children and an owned link stand-in, so they call no Windows API
or product. Run those portable tests before the existing actual Oracle gate.

The initial unpopulated sparse Git index also produced a bad unpublished
retention commitb2ef6ecc with unintended deletions. Its exact state remains on
the separate `issue/805-windows-owner-control-empty-index-proof` branch and in
the original evidence map. Restore the full parent index before the corrected
retention commitea35f2d5; that bad commit is not an ancestor of this candidate.
Original worktrees and product sources were unchanged throughout preparation.

The isolated worktree uses sparse checkout solely to respect the shared disk
floor. Its Git index retains the full parent source. Disable sparse checkout
under an allocated workspace budget before future full compilation; there is
no target in this worktree and no compiler or port allocation in this phase.
