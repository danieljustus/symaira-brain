---
name: golden-fixture-codex
description: A canonical fixture skill for per-target golden render tests
license: Apache-2.0
compatibility: codex
metadata:
    source: canonical
---

# Golden Fixture

This fixture skill is used by per-target golden tests to assert that
rendered skill bundles match each harness contract.

It exercises:
- Frontmatter rendering with target-specific aliases
- Support file copying (scripts directory)
- Overlay application (opencode prepend)
- Codex metadata generation (agents/openai.yaml)
