# Grok recorded local history: consumer contract

This is an additive optional schema-1 capability in **Brain**, not a Cockpit UI implementation.

## User flow

```sh
SYMBRAIN_USAGE_GROK_HISTORY=1 symbrain usage --providers grok --output json
```

`--json` and table output also work. Only the exact opt-in value `1` enables history. It does not select Grok: omit `--providers grok` and the historical ten-provider default graph stays unchanged. Collection happens at report time, before quota work, only for a selected Grok provider. There is no history construction-time I/O. The historical no-auth/default JSON contract is unchanged without opt-in. Quota credential discovery retains its existing behavior; enabling history does not sign in, refresh or write credentials.

Read `$GROK_HOME/sessions/**/updates.jsonl`, else `$HOME/.grok/sessions/**/updates.jsonl`. GROK_HOME must be absolute, with no `.`/`..` components or symlinked ancestor. Home expansion (`~`) is not performed. All files are read-only. Debug logs, summaries and prompts are not output. No pricing downloads or inferred prices exist in this slice.

## Cockpit decoding

Keep `schema_version == 1`, all legacy required fields, existing decimal strings and `snapshot`/`error` semantics. Decode the new **optional `providers[].history`** independently of `configured`, `auth_status`, `snapshot` and `error`. A missing field means **not requested/not supported**, never zero. Missing credentials and cloud quota errors can coexist with usable history. Do not place history in quota meter labels, snapshot source or balance.

History object, all fields required when present:

| Field | Type / meaning |
|---|---|
| `scope` | `"local_machine"`; not cloud billing or balance |
| `attribution` | `"unattributed"`; never the current signed-in account |
| `timezone` | `"local"` in production, meaning the Brain host's calendar/time zone, **not the consumer's**; injected library tests may name another zone |
| `status` | `available`, `no_data`, `partial`, `unavailable` |
| `excluded_events` | integer count of rejected lines, unfinished non-LF tails, or conflicting event/model identities; diagnostics, not an exact count of lost turns |
| `daily` | ascending `{key: "YYYY-MM-DD", models: [...]}` rows, only dates with accepted data |
| `periods` | ascending key rows: `last_30_days`, `today`, `yesterday`, only periods with accepted data |

Each model row has required `model`, `events`, and `missing_tokens`, with optional decimal-string `tokens`. The total is omitted if any accepted contribution lacks a token measurement. Genuine measured zero is `"0"`; empty-day rows are not fabricated. `partial` signals incomplete data. `unavailable` has empty arrays and means an admission, budget or read failure. Monetary fields are not read or emitted.

Calendar window: today plus the preceding 29 local dates; yesterday is the preceding calendar date, not the preceding 24 hours. Future timestamps are excluded. DST is resolved by the host time-zone rules at each event. Model rows sort lexically; daily rows sort ascending. Consumers should select period keys, not array positions.

## Event acceptance and ownership

Fixtures here are explicitly synthetic source-shaped controls, not native Grok captures.

Accept completed `update.sessionUpdate == turn_completed`, in `params` or top-level envelope. Prefer agent millisecond timestamp, then outer RFC3339 or integer Unix seconds. Per-model `usage.modelUsage` contributions use `inputTokens + outputTokens`; cached read/creation and reasoning are subsets, not added again. Exact nonnegative integer counts only, input/output each at most 10^12. Missing, negative, fractional, non-integer-string, boolean or oversized measurement values remain unknown; they are not clamped to zero.

Deduplicate by stable `_meta.eventId` and model across every session kind, including nested subagents, resumed/forked copies. Different models within one event stay separate. Conflicting copies are excluded, not first-wins. Missing event ID means ambiguous deduplication and is excluded/flagged. This is intentionally stricter than upstream's count-every-no-ID behavior. No observed event field establishes ownership by the current login; all history stays explicitly unattributed. Account-filtered history and cloud reconciliation remain unsupported.

## Bounds and safety

Unix: retain directory capabilities, reject symlinked ancestors/children, open files no-follow/nonblocking, verify regular-file type on the opened handle. Unsupported platforms return `unavailable` before accessing history. Limits: depth 12 below sessions, 4,096 traversal entries, 128 ledgers, 4 MiB/ledger, 32 MiB total payload, 20,000 nonempty lines, 256 KiB/line, 32 models/event, 10,000 model contributions before deduplication. Any exceeded hard limit discards the whole scan. A cooperative two-second traversal/parser deadline plus report cancellation bounds normal work, **not blocked kernel regular-file I/O**. Aggregation is bounded by the contribution cap. No persistent cache, source mutation or atomic multi-file snapshot is claimed. An unterminated final line is excluded/flagged even if syntactically valid; a later refresh admits it once newline-terminated.

Native synthetic tests and isolated CLI smoke establish implementation behavior, not genuine Grok/account acceptance or all-platform security certification. No live credential/provider calls are part of these gates.
