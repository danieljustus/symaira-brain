---
name: golden-fixture-open
description: A canonical fixture skill for per-target golden render tests
license: Apache-2.0
compatibility: opencode
metadata:
    source: canonical
---

> This overlay prepend fragment applies exclusively to the **opencode** harness.
> It is inserted before the main body during rendering.

# Golden Fixture

This fixture skill is used by per-target golden tests to assert that
rendered skill bundles match each harness contract.

It exercises:
- Frontmatter rendering with target-specific aliases
- Support file copying (scripts directory)
- Overlay application (opencode prepend)
- Codex metadata generation (agents/openai.yaml)
