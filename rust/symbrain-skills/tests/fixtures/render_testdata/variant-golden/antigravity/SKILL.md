---
name: variant-fixture
description: A canonical fixture skill exercising harness variants per target
license: Apache-2.0
compatibility: antigravity
metadata:
    source: canonical
---

# Variant Fixture

This fixture exercises the harness-variant constructs so each target's
resolved output is reviewable as a committed file.

## Worker execution

Run each group in its own worktree and verify it before integration.

## Reporting

Persist the run report under ~/.local/state/symskills/reports/variant-fixture/.


Use whatever isolation mechanism this harness provides.
