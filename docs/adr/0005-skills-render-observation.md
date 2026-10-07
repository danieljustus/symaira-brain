# ADR 0005: Preserve render observations when installation comparison fails

Status: implementation decision for issue #621; corrected code requires independent review and fresh native acceptance.

## Context

Skill status compares the installed tree against its frozen base and a fresh library render. The retained render cache is also observable state: users can edit reference files through managed links. Independent review of PR #797 at `ddd34beee51ef83bc0d115f3ad6b3038b7a970aa` reproduced a missing render report when the ordinary three-way comparison rejected a linked cache containing a nested link, FIFO, or excessive depth. Copied installations already retained an explicit unreadable cache report, so the two delivery modes differed.

## Decision and rationale

Once a fresh target render exists, construct the render observation independently of whether installation comparison succeeds. Preserve the original stale classification, diagnostic and summary when that comparison fails, while retaining `render_status=unreadable` and its reason if the cache cannot be read safely. This keeps clients informed about the separate retained cache without changing synchronization or repair policy.

Reject unsafe cache contents and report no cache hashes on a failed scan. Present a managed link as `linked` only after the existing safe comparison establishes its exact render-cache relationship; an unreadable tree retains its persisted `symlink` mode. Status writes no library, render, base, marker, or installation state and never promotes cached edits into the skill SSOT. Users retain control over preserving those edits in the library.

Keep the original independent review and failed actual CLI observations under `migration/evidence/skills-render-drift-621/`. New regressions exercise actual managed cache links with unsafe contents, both JSON and table output, unchanged stale summaries, and before/after filesystem snapshots. Unix additionally exercises a FIFO; native Windows and macOS acceptance remains mandatory. Tests for ordinary copied caches remain in place.

## Consequences

Consumers receive the render diagnostic even when three-way classification fails after a fresh render is available. Existing installation errors keep their meaning. A failure to load or materialize the library before any fresh render exists still uses the ordinary status error path. This is a scoped observational correction, not completion of #764 or the full Rust cutover. PR #794 must land first; the integrated corrected head needs independent review and fresh native CI before merge.

## Integration after the bounded preflight merge (2026-10-04)

PR #794 merged as `9353520a34c5819c7d0a6cd58d3bfaa2b22b045b`; issue #793 is closed. Integrate that exact main commit by a normal merge, preserving the previous #621 head `4f3c0032b122c29804ccb0aea674202a5165f7a6` and its original runtime evidence. Retain the reviewed raw-byte/WTF-8 Skills sync flag implementation from main in full. Render observations are an additive status feature and do not justify reverting platform-correct argument handling.

Keep both the #621 render-drift and #794 Windows argv workflow groups, their independent artifact uploads, and main's Activity checks. Preserve historical diagnostics and handoff documentation rather than rewriting old source-only observations as current acceptance. The contract matrix keeps main's `SKL-008` Windows argv identifier and assigns the new render observation `SKL-009`, so independent contracts do not share a key. Original merge conflicts are retained with full bytes and SHA bindings under `/workspace/review-proof/root-skills621-main935-integration/`.

The merged source keeps the complete #621 status/render implementation and tests byte-identical to its previously reviewed head; its shared CLI additionally inherits main's bounded preflight corrections. Source checks at the integrated head and fresh actual Linux/macOS/Windows CI are required. Earlier runtime reports remain acceptance only of their recorded heads; this integration does not close #764 or establish full Rust cutover.
