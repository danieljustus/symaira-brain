# Library and render drift (#621)

The native `skills status --target <harness>` report compares an existing
`rendered/<harness>/<skill>` tree with a fresh target render from the library.
The JSON adds `render_status` (`in-sync`, `drift`, or `unreadable`), changed
paths and SHA-256 hashes in `render_drift`, and a `render_error` when comparison
fails. Tables add `RENDER` when any row has a retained render. Target transforms
are applied before comparison, so ordinary transformed `SKILL.md` files do
not count as edits.

A verified managed symlink onto that exact render has presentation mode
`linked`. Its existing three-way status still reports `harness-changed` or
`conflict` when appropriate. Marker modes and synchronization policy retain
their existing meanings. Status never imports render edits into the library
or attempts to repair a link; review and preserve those edits in the library
before re-rendering. Copy installations report render drift independently of
their installation status, so an unchanged installed copy cannot hide edits
made only to cached reference files.

The new fields are omitted when no render is retained. Explicit cached targets
can use the native report, including the legacy `symskills` data root. Dynamic
skill configuration and unqualified multi-target CLI gates remain within #764.
Directory capabilities, existing shared locks, bounded hashing (including a 32-level directory-depth limit), and rejection
of nested links and special files remain in force. An unreadable cache is an
explicit failure to compare, never an `in-sync` result.

Run the real process check with pinned Go and Rust available:

```sh
scripts/skills-render-drift/run.sh /tmp/skills-render-drift.json
```

It builds the entire immutable Go CLI at
`dcddcef0df5789123c7c9a7ebe6e01f10e941f2c`, then runs 12 clean/edited reports
across all six targets in disposable HOME/XDG roots. Native children have an
empty PATH and no fallback binary. The comparison removes only the documented
new fields and maps the verified presentation mode back to `symlink`; every
remaining report field, success status, and stderr must agree with actual Go.
The report retains both outputs, source/binary hashes, candidate head, and
runtime. Ordinary Rust tests still exercise the full new behavior without Go;
live mode requires the oracle and exactly 12 distinct cases. Native CI runs
this check on Linux, macOS, and Windows and retains its report. No Go source or
existing oracle output is edited, and the extensions are intentional product
changes rather than byte-parity claims for the complete new report.

The source-bound Linux checkpoint and actual missing-oracle / mutated-status
failure controls are in `migration/evidence/skills-render-drift-621/`. Both
controls fail for their intended reason; CI acceptance remains pending until
the native jobs succeed at the published candidate head.

This feature depends on the bounded marker/preflight repair in PR #794. Its
final branch includes that exact repaired head; do not merge the dependent PR
until #794 receives its required independent review and lands in main, then
verify the integrated candidate with fresh native CI.
