# Current Claude and OpenRouter usage contract

These `*-current-cases.json` examples are hand-authored offline regression inputs,
**not live captures or migration-oracle evidence**. Their crate-local Rust copies
are byte-compared by `TestCurrentFixtureCopiesMatch`; Rust embeds only files inside
its publishable crate. The original fixtures/oracle files and provenance checks
are unchanged.

Behavior reference: OpenUsage (MIT), commit
`abf472d5dbb23645c3afbb5a0c7429606ffbdcd7`, particularly ClaudeUsageMapper and
OpenRouterUsageClient. Only payload semantics were adopted; no upstream code was
executed and no implementation was copied wholesale.

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

## OpenRouter

- Required `/credits`: nonnegative finite numeric `total_credits` and
  `total_usage`; balance is their signed difference. Separate lifetime credit
  and spend meters retain account-wide scope.
- Best-effort `/key`: Today / This Week / This Month use `usage_daily`,
  `usage_weekly`, `usage_monthly`. Key-cap usage is `limit - limit_remaining`,
  never lifetime `usage`. Missing cap usage stays absent; numeric zero survives.
- Invalid optional payloads, network errors, authentication failures and rate
  limits retain valid account credits. Optional lookup has a one-second budget
  and reserves 100ms of the provider deadline to publish the required snapshot.
- `/auth/key` is retained **only after `/credits` returns 404**. Authentication,
  server, transport, and schema errors do not downgrade to legacy data.
- Existing validated HTTPS transport, response bounds and no-redirect policies
  remain in force; new required transport errors do not echo arbitrary details.

## Deliberate impact on frozen migration evidence

The original Go oracle describes a single `/auth/key` request. It cannot certify
this new two-response contract. Do not rewrite original oracle payloads/hashes,
weaken provenance validation, or disguise its old key payload as `/credits`.
The new shared offline cases explicitly assert the new request sequence,
normalized meters, zero/absence, failure isolation and 404-only legacy fallback.

Historical key-only Go observations remain unchanged. The historical request,
report, and provider-graph tests now explicitly inject `/credits` HTTP 404,
assert the new prelude (URL/method/all headers/body), and compare the remaining
legacy request and complete report against their original expectations. This
is legacy-fallback coverage, not current account-credits certification.

Current OpenRouter acceptance is separate: `scripts/openrouter-oracle-v1`
records actual production Go constructor/report execution with ordered scripted
responses. The 27-case capture is bound to immutable source
`f0fb64902ba55cc3bc046af992df9ee2e81ebfb6`; the evidence/test commit is later.
`openrouter_go_v1.json` is the exact captured output, not rewritten expectations.
Rust compares complete reports, errors and every request/header/body directly.
The differential exposed a real wrong-type error-taxonomy mismatch, repaired in
Rust without changing the Go capture. Provenance, deterministic recapture,
mutation rejection and full crate verification remain separate checks.
No live endpoint behavior is certified.

## Remaining parity gaps (outside this slice)

No Claude token refresh/writeback, profile/plan metadata, Desktop/multi-account
identity, local session-spend history, reset-grant listing or reset consumption.
No OpenRouter local/history/account identity or plan metadata. No provider
registry/selection/CLI-routing changes, new providers, persistent quota cache,
pricing/model attribution, Cockpit changes, or live provider verification.
Schema remains version 1. Known model display names are intentionally bounded;
unknown future model names require an explicit contract update.
