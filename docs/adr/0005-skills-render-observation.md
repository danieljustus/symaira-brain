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
