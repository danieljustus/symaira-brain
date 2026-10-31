# Current Claude usage contract

These `*-current-cases.json` examples are hand-authored offline regression inputs,
**not live captures or migration-oracle evidence**. Their crate-local Rust copies
are byte-compared by `TestCurrentFixtureCopiesMatch`; Rust embeds only files inside
its publishable crate. The original fixtures/oracle files and provenance checks
are unchanged.

Behavior reference: OpenUsage (MIT), commit
`abf472d5dbb23645c3afbb5a0c7429606ffbdcd7`, particularly ClaudeUsageMapper. Only
the documented payload field meanings were used; the implementation is our own
and no upstream code was executed or copied.

## Claude

- `utilization` is already a percentage: use a 100% limit regardless of a legacy
  `limit`. Zero is measured zero; missing utilization is not zero.
- Legacy `utilized` with a positive `limit` remains supported. A missing legacy
  utilized value now leaves `used` absent instead of inventing zero.
- `limits` entries with `kind: weekly_scoped` and model display name Fable,
  Sonnet or Opus emit independent model quotas and their own reset time. A scoped
  model supersedes only its matching legacy `seven_day_<model>` window.
- Current enabled extra usage reads `used_credits` / `monthly_limit` in cents and
  emits USD; disabled extra usage is omitted. A zero monthly cap means uncapped.
  Legacy `used` / `limit` remain USD. Over-100% utilization is not clamped.

## Remaining parity gaps (outside this slice)

No Claude token refresh/writeback, profile/plan metadata, Desktop/multi-account
identity, local session-spend history, reset-grant listing or reset consumption.
No OpenRouter account credits, spend meters or key period meters: this repository
reports token usage, not costs. No provider
registry/selection/CLI-routing changes, new providers, persistent quota cache,
pricing/model attribution, Cockpit changes, or live provider verification.
Schema remains version 1. Known model display names are intentionally bounded;
unknown future model names require an explicit contract update.
