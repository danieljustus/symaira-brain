---
name: variant-fixture
description: A canonical fixture skill exercising harness variants per target
license: Apache-2.0
compatibility: hermes
metadata:
    hermes:
        tags:
            - Fixture
    source: canonical
---

# Variant Fixture

This fixture exercises the harness-variant constructs so each target's
resolved output is reviewable as a committed file.

## Worker execution

Run each group in its own worktree and verify it before integration.

## Reporting

Persist the run report under ~/.hermes/reports/variant-fixture/.

Provider-backed children are allowed; provider CLIs are not.

