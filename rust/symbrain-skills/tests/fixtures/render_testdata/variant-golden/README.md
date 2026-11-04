# Harness-Variant Golden Fixtures

Committed per-target render output for `TestGoldenVariantRender` in
`internal/render/golden_variant_test.go`. Where `testdata/golden/` pins the
render pipeline's *structure* (frontmatter, support files, install paths),
these trees pin what the harness-variant constructs actually *resolve to* —
in a form where a change to any harness's text shows up as a reviewable diff
rather than only as a passing or failing assertion.

## What the fixture exercises

The source at `internal/render/testdata/variant-source/` combines all four
constructs in one skill:

| Construct | Where | Effect |
|---|---|---|
| `symskills:block worker-execution` | `SKILL.md` | Overridden for `claude` only; every other target keeps the canonical text. |
| `symskills:block dispatch` | `references/execution-contract.md` | Overridden for `claude` and `hermes`, proving resolution reaches reference files. |
| `symskills:only hermes` | `SKILL.md` | Present in the hermes tree, absent from the other five. |
| `symskills:except hermes` | `SKILL.md` | Absent from the hermes tree, present in the other five. |
| `{{term:report_dir}}` | both | `~/.hermes/reports` for hermes, the default elsewhere. |
| `metadata.hermes` | frontmatter | Present in the hermes tree only. |

## Regenerating

    go test ./internal/render/... -run TestGoldenVariantRender -update

`TestGoldenVariantInvariants` runs against these same trees and asserts the
properties that must hold whatever the goldens contain — no leftover markers,
no unresolved placeholders, no shipped overlay input, and the correct
scoping of the only/except regions. A careless `-update` cannot bless output
that violates them.
