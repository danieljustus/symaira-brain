---
name: variant-fixture
description: A canonical fixture skill exercising harness variants per target
license: Apache-2.0
compatibility: claude
metadata:
    source: canonical
---

# Variant Fixture

This fixture exercises the harness-variant constructs so each target's
resolved output is reviewable as a committed file.

## Worker execution

Dispatch each group with the Agent tool, one subagent per group, each in its
own git worktree under `<repo>/.worktrees/<task-slug>`.

## Reporting

Persist the run report under ~/.local/state/symskills/reports/variant-fixture/.


Use whatever isolation mechanism this harness provides.
