# Canonical Codex local history (additive schema 1)

## Activation and privacy boundary

`SYMBRAIN_USAGE_CODEX_HISTORY=1` **and explicit** `--providers codex` (or a validated comma-separated selection containing `codex`) enable history. Values such as `true`, an omitted selector, or selecting other providers do not. Construction never scans history.

Additionally setting exact `SYMBRAIN_USAGE_CODEX_HISTORY_ONLY=1` under both gates constructs Codex without quota credential discovery/resolution or requests. It ignores even a present credential reference and reports ordinary missing authentication, not fabricated configuration. The flag alone changes nothing. Other selected providers retain their own behavior; selecting only Codex is the credential-free launch contract. Without local-only mode, existing quota behavior remains, and history survives quota errors/missing credentials independently.

One native absolute `CODEX_HOME` is respected, default `$HOME/.codex`. Only `sessions/**/*.jsonl` and `archived_sessions/**/*.jsonl` are read. Active relative paths shadow matching archived relative paths. No direct-home fallback, multiple homes, symlinks or implicit login/account discovery. Nothing reads real user logs during verification. No config, auth or message contents appear in reports.

History is `scope: local_machine`, `attribution: unattributed`: neither the rollout nor current login proves historical account ownership.

## Wire contract

`providers[].history` uses the existing typed `LocalHistory` and optional recorded-versus-estimated additions documented in `usage-claude-local-history.md`. Report/schema/provider identifiers are unchanged. Tokens and costs are decimal **strings**, not floats. Each model row has coverage counts, `cost_basis` (`recorded`, `estimated`, `mixed`, `unknown`), optional `recorded_cost` and `estimated_cost`, `recorded_events`, `estimated_events`, `missing_tokens`, `missing_cost`. Subtotals are disjoint. Missing cost is never zero; API estimates are never labeled billed/recorded. Optional `price_snapshot` identifies the frozen table even when a particular model is unpriced.

Common aggregation emits populated daily dates and `today` / `yesterday` / `last_30_days` for today and the preceding 29 local dates, excludes future events and does not fabricate empty days. Production timezone is `local`; fixture APIs accept an injected instant/zone. Existing Grok/OpenCode recorded-only output is unchanged.

Canonical Codex rollouts have no recorded charge field in the inspected source. They produce estimates only. The additive top-level numeric `costUSD` compatibility extension (shared with the existing recorded-cost contract, **not claimed as a canonical Codex field**) is authoritative, including zero. A malformed present value suppresses estimation, including explicit null. Exact lexical decimal decoding and the existing cost aggregator are reused, not a new pricing framework.

## Normalization and deduplication

Inspected read-only upstream: OpenUsage revision `abf472d5dbb23645c3afbb5a0c7429606ffbdcd7`, `CodexLogFileParser.swift`, `CodexLogUsageScanner.swift`, `CodexUsagePricing.swift`, `CodexLogUsageScannerTests.swift`, `TestSupport.swift`, provider docs and bundled snapshots. Source-shaped fixtures are synthetic controls, not upstream execution or real user captures.

- Track model from `turn_context` and inline token-count payload/info (`model`, `model_name`, `metadata.model`); missing model is visible `unknown`, **never** an implicit GPT-5 or user fallback model.
- Read `event_msg` / `payload.type=token_count`. Null/missing `info` carries no measurement. Prefer `last_token_usage`; otherwise subtract the previous `total_token_usage`. Null last/total objects are absent. Unchanged cumulative snapshots are not new turns, even with a repeated last-usage payload. Zero usage does not create an event or a priced zero turn.
- Input includes cached input; output includes reasoning. Tokens are **input + output**, not input + cached + output + reasoning. Pricing uses `(input - cached) * input_rate + cached * cache_read_rate + output * output_rate`. Optional `total_tokens` must agree. Missing cache/reasoning subsets mean zero; missing input/output, malformed counters, counters over 10^12 and inconsistent subsets are excluded. A malformed usage record poisons later measurements in that file (they remain incomplete, not confidently repriced), because the delta baseline may be lost.
- Each file owns independent state. Last-only records advance a virtual cumulative baseline once per file-local event fingerprint, so a later cumulative-only snapshot does not count their usage again; copied last-only records do not advance it twice. Any counter regression starts a new cumulative epoch: use explicit last usage when present, otherwise the whole new snapshot. These intentionally correct the pinned parser's gap double-counting and componentwise-clamp reset loss. They cannot prove why Codex reset a counter.
- First session metadata identifies fork/subagent markers (`forked_from_id`, `parent_thread_id`, `thread_source=subagent`, `source.subagent`). Replayed parent usage seeds totals but is never charged to the child. A `task_started.started_at` at/after the child's creation wall-clock second opens the live gate. Invalid/missing creation time uses that task-start line's own second. Null/blank parent markers do not make a root a child. A child with no qualifying live start remains excluded from chargeable usage.
- Copied events deduplicate by session ID (when supplied), UTC instant, original model, normalized usage, cumulative snapshot and historical service tier. Distinct explicit sessions remain distinct even at the same timestamp with equal counts. Without a session ID the source-compatible timestamp/count fingerprint is used; independently generated identical no-ID events cannot be distinguished. Session IDs/fingerprints never leave the scanner.

## Frozen estimates

Identical MIT-noticed assets `internal/usage/codex_prices.json` and `rust/symbrain-usage/src/codex_prices.json` contain exact integer **10^-10 USD/token** rates and source hashes. Snapshot:

`openusage:abf472d5dbb23645c3afbb5a0c7429606ffbdcd7:litellm-2026-07-02:supplement-2026-10-16:modelsdev-2026-07-02:codex`

The LiteLLM snapshot supplies base rates, the pinned supplement overrides published models and aliases, models.dev supplies exact cross-source decimal rates where available. Nonintegral binary-conversion artifacts without an exact counterpart are not rounded or invented: those entries stay unpriced. Finite model/effort aliases are derived from the supplement rules; unknown aliases do not acquire a fallback model. Source retrieval timestamps are retained through the original source hashes and snapshot identity (LiteLLM/models.dev `2026-07-02T12:43:06Z`, supplement `2026-10-16T23:28:55Z`). Full MIT notice is embedded in both assets.

Published Codex-specific >272k whole-request rates, cache-discount exceptions, fast/priority multipliers (including exact 2.5x for GPT-5.5), and GPT-6 Astra Ultrafast 6x come from the pinned `CodexUsagePricing.swift`. Service tier is tracked from historical `thread_settings_applied` (nested or direct `service_tier`), never current `config.toml`. Missing tier means standard; unknown tier retains tokens with missing cost. Fast aliases normalize to base rates before applying the multiplier once. `gpt-reserve` prices as pinned Luna; Daybreak aliases use pinned Sol. Unknown models retain tokens with missing cost/partial status. Auto-review's dated/free-era policy is not implemented: its measured tokens stay unpriced, not guessed free.

All amounts are frozen API-rate estimates, not subscription charges, current prices, historical account billing, or verified bills. No runtime price network, refresh, download or paid request exists. Per-event costs above 10^15 price units are unknown; total accepted events are capped to keep exact existing accumulators within u64.

## Native bounds

The shared no-follow, capability-relative reader is retained. No-follow directory opens apply at every ancestor, and nonblocking no-follow final opens require regular files; symlinks, FIFOs and sockets fail closed. Unsafe discovery/limits yields `unavailable` with no published partial prefix; malformed complete JSONL records or an unfinished final line produce `partial` diagnostics. This is a cooperative deadline, not a kernel-enforced read timeout or protection against arbitrary same-user mutation of regular-file contents.

Per `sessions`/`archived_sessions` subtree: depth 12, 4096 entries, 128 eligible files, 4 MiB/file, 32 MiB total, 20k complete nonempty lines, 256 KiB/line. Combined: at most 8192 discovered entries, 256 files, 64 MiB, 40k lines, **10k accepted events**, shared two-second cooperative deadline plus caller cancellation/deadline. Active-path shadowing skips duplicate archive payloads without reading them. An absent subtree is no data; non-Unix scanning is unsupported/fail-closed.

## Explicitly unsupported

pi logs/auth, xswap/Swap homes or shared-history links, multiple Codex homes, synced/symlink roots, OpenCode ChatGPT OAuth attribution, account/workspace ownership, direct-home scan fallback, ephemeral/nonpersisted usage, fallback-model UI/config (remains **None**), auto-review dated/free pricing, network price refresh, live credentials and provider calls. This is canonical native-rollout support, **not full OpenUsage Codex-family parity**. Other platforms require native verification; macOS evidence does not certify Linux/Windows runtime behavior.
