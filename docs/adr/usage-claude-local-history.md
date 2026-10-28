# Claude Code local history: additive consumer contract

Schema 1 remains unchanged. This is an opt-in local-machine report, not account billing or full OpenUsage Claude-family parity.

## Activation and isolation

`SYMBRAIN_USAGE_CLAUDE_HISTORY=1` AND explicit `--providers claude` (possibly in a validated comma-separated selection) enable `providers[].history`. Nonexact values and the historical default provider graph do not scan. Construction never scans history. Optional `SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY=1`, effective only with both gates, disables Claude quota credential discovery and quota requests; its auth status remains missing. Without history-only, existing quota credentials and errors remain independent of history. Other providers are not constructed when not selected.

Source: one absolute `CLAUDE_CONFIG_DIR` config directory, otherwise `$HOME/.claude`; read only its `projects/**/*.jsonl`. Comma-separated roots, direct projects aliases, XDG fallback, Desktop/Cowork/Swap auto-discovery, pi, Conductor, SDK-specific roots, advisor iterations and nonpersistent sessions are unsupported. Explicitly selecting a config directory does not prove account ownership: scope stays `local_machine`, attribution `unattributed`.

## Records and deduplication

Accept LF-terminated JSON records with top-level `type: assistant`, nested `message.role: assistant`, a nonempty bounded message ID and requestId, RFC3339 timestamp and valid model slug. Explicit `message.stop_reason` values are limited to `end_turn`, `tool_use`, `max_tokens`, `stop_sequence`. Canonical persisted Claude Code assistant records may have a null/missing stop_reason: like the pinned scanner, accept those when input_tokens and output_tokens are present nonnegative integers. These are complete LF-terminated recorded usage contributions, **not proof that the whole turn terminated**. Message-start fragments without usage and non-assistant streaming events contribute nothing; incomplete final lines are excluded. Multiple persisted assistant content chunks reuse a message ID and dedup to the preferred token total. Parent and nested subagent files are unioned. Resume/fork copies with the same `(message.id,requestId)` count once; a sidechain copy of the same message under another request also collides. Prefer non-sidechain, then larger total tokens, then a usable recorded cost, then explicit speed, then first path/line. Distinct non-sidechain requests remain distinct. Different models on a colliding identity are excluded rather than assigned an invented model. Dedup precedes calendar filtering.

Require input/output nonnegative integer buckets; omitted cache read/write buckets mean zero as in the pinned Claude format, explicit null/invalid means missing. Cache creation split (5m/1h) replaces, never adds to, its aggregate; inconsistent split and aggregate makes tokens/cost unknown. Unknown speed/service_tier or unsupported nested advisor iterations retain main-model tokens but suppress estimates and mark partial. Recorded `costUSD`, including zero, is authoritative; invalid present cost does not fall back to an estimate.

## Exact output and money

Existing daily/period keys, sorting, local calendar dates (today and preceding 29 dates), future exclusion and missing-day behavior are reused. Amounts are decimal strings in USD, never binary floating-point sums. No prompt/text, request/session IDs, source paths, credentials or content enter output.

Optional additive fields (omitted for Grok/OpenCode):
- History `price_snapshot`: stable bundled snapshot ID including pinned source revision and supplement/catalog timestamps.
- Model `estimated_cost`: sum of successfully estimated contributions only; never written into `recorded_cost`.
- Model `cost_basis`: `recorded`, `estimated`, `mixed`, or `unknown` for that row.
- Model `recorded_events` and `estimated_events`: counts defining the corresponding subtotal coverage. For Claude only, `recorded_cost` is the recorded subtotal when recorded_events > 0, estimated_cost the estimated subtotal when estimated_events > 0. Neither is an account-wide total. Missing contributions do not disappear: required `missing_cost` counts events with neither usable recorded cost nor usable estimate. A row with missing_cost > 0 is incomplete even when one subtotal exists. `tokens` retains the existing all-contributions completeness rule.

History `cost_basis` is `recorded`/`estimated`/`mixed` over usable in-window contributions, or `unknown` if none has usable cost. `partial` covers excluded events and missing tokens/cost. Exact zero is `"0"`; an absent amount is not zero. Never combine the subtotals into a field labeled recorded or billed. Grok/OpenCode keep their previous all-recorded completeness semantics and byte shape.

## Offline pricing provenance

The small Claude-only bundled table is derived from OpenUsage MIT-licensed resources at `abf472d5dbb23645c3afbb5a0c7429606ffbdcd7`, `pricing_litellm_snapshot.json` retrieved `2026-07-02T12:43:06Z`, `pricing_models_dev_snapshot.json` retrieved `2026-07-02T12:43:06Z`, and `pricing_supplement.json` updated `2026-10-16T23:28:55Z`. The accompanying MIT notice is retained both beside and inside the embedded asset. Source-byte SHA-256 values are recorded in the asset. Haiku 4.5 cache-read `0.09999999999999999` in the LiteLLM snapshot is not rounded: the exact `0.1` from its `anthropic/claude-haiku-4-5` models.dev entry is used explicitly, with LiteLLM’s 1.25 cache-write rate retained. No framework, downloader, network refresh or user-specific rate is imported. Rates represent frozen API-rate estimates, not current prices, historical invoices or actual subscription charges. Known dated aliases are explicit; no fuzzy unknown-model fallback. Unsupported precision/rates remain unknown. 1h cache writes use twice the selected input rate per the pinned `ModelRates.swift`; long context selects whole-request tiers strictly above 200,000 prompt tokens; fast mode requires an explicit model multiplier or explicit fast rate. Details and exact table contents live beside the implementation.

## Bounds

Native capability-relative no-follow directory/file opens, regular-file admission and nonblocking flags reject symlinks/FIFOs. At most depth 12, 4,096 entries, 128 JSONL files, 4 MiB/file, 32 MiB total, 20,000 nonempty complete lines, 256 KiB/line, 10,000 accepted records, bucket <= 10^12. A two-second cooperative scan deadline is not a kernel-read hard timeout. Hard bounds/admission failures discard the scan as unavailable; malformed complete lines and partial trailing records are counted as excluded. No persistent cache, source writes or source SQLite access.

All new execution fixtures are synthetic documented-schema controls, not authentic user logs. Earlier immutable acceptance captures remain unchanged and do not certify these uncommitted changes.
