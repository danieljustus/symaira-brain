# Preserve current checks while retiring obsolete queued work

Status: accepted coordination decision,2026-10-04. The user delegates technical
choices and requests their rationale in docs. This changes no required check,
coverage floor, assertion, timeout, runner selection or merge protection.

Cancel only an obsolete queued Actions run whose associated open PR currently
has another head and whose fresh job census contains no running job. Nine cancel
requests were accepted; five other candidates were skipped when their fresh
state no longer qualified. An accepted cancellation is asynchronous and is not
itself proof of a final cancelled status. Current-head and running jobs remain
protected. Completed jobs, raw failure logs and artifacts are retained.

This reduces stale runner demand while preserving the exact-head evidence needed
for a normal merge. Reusing old successful checks or bypassing queued macOS/gui
jobs would not establish the proposed commit's behavior. The original run/job/
PR-head observations, script and per-candidate decisions are SHA-bound in
`migration/evidence/memory-ui-763/independent-b7fb40f/retention.json`.

Build outputs have one explicit owner. Reassign an idle ignored cache only after
its owner releases it, a fresh proc exe/cwd/fd/maps census finds no users, and all
ELF payloads, including objects and build scripts, have lossless SHA-verified
archives. Preserve actual execution/source receipts separately from cached
objects; a cached file is not a current-source runtime observation.

The released Memory763 cache moved from its former UI target to Doctor649:
523 ELF paths/489 unique byte sequences were verified against retained archives,
with no new binary archive needed and no source changes. The previous source,
original failures and actual executed binaries remain intact. Doctor649 receives
this cache exclusively but cannot claim its newly combined parent is validated
until fresh affected tests and process comparisons execute. Serial heavy builds
and a700MiB free-space boundary prevent an avoidable partial build from consuming
the workspace's last capacity. Compiler allocation is separate from cache
ownership, and no original preexisting target is retired.
