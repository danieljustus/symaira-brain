---
name: variant-fixture
description: A canonical fixture skill exercising harness variants per target
license: Apache-2.0
metadata:
  source: canonical
  hermes:
    tags:
      - Fixture
---

# Variant Fixture

This fixture exercises the harness-variant constructs so each target's
resolved output is reviewable as a committed file.

## Worker execution

<!-- symskills:block worker-execution -->
Run each group in its own worktree and verify it before integration.
<!-- /symskills:block -->

## Reporting

Persist the run report under {{term:report_dir}}/variant-fixture/.

<!-- symskills:only hermes -->
Provider-backed children are allowed; provider CLIs are not.
<!-- /symskills:only -->

<!-- symskills:except hermes -->
Use whatever isolation mechanism this harness provides.
<!-- /symskills:except -->
