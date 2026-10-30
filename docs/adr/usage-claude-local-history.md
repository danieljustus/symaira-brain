# Claude Code local history: additive consumer contract

Schema 1 remains unchanged. This is an opt-in local-machine report, not account billing.

## Activation and isolation

`SYMBRAIN_USAGE_CLAUDE_HISTORY=1` AND explicit `--providers claude` (possibly in a validated comma-separated selection) enable `providers[].history`. Nonexact values and the historical default provider graph do not scan. Construction never scans history. Optional `SYMBRAIN_USAGE_CLAUDE_HISTORY_ONLY=1`, effective only with both gates, disables Claude quota credential discovery and quota requests; its auth status remains missing. Without history-only, existing quota credentials and errors remain independent of history. Other providers are not constructed when not selected.

Source: one absolute `CLAUDE_CONFIG_DIR` config directory, otherwise `$HOME/.claude`; read only its `projects/**/*.jsonl`. Comma-separated roots, direct projects aliases, XDG fallback, Desktop/Cowork/Swap auto-discovery, pi, Conductor, SDK-specific roots, advisor iterations and nonpersistent sessions are unsupported. Explicitly selecting a config directory does not prove account ownership: scope stays `local_machine`, attribution `unattributed`.

## Records and deduplication

Accept LF-terminated JSON records with top-level `type: assistant`, nested `message.role: assistant`, a nonempty bounded message ID and requestId, RFC3339 timestamp and valid model slug. Explicit `message.stop_reason` values are limited to `end_turn`, `tool_use`, `max_tokens`, `stop_sequence`. Canonical persisted Claude Code assistant records may have a null/missing stop_reason: like the pinned scanner, accept those when input_tokens and output_tokens are present nonnegative integers. These are complete LF-terminated recorded usage contributions, **not proof that the whole turn terminated**. Message-start fragments without usage and non-assistant streaming events contribute nothing; incomplete final lines are excluded. Multiple persisted assistant content chunks reuse a message ID and dedup to the preferred token total. Parent and nested subagent files are unioned. Resume/fork copies with the same `(message.id,requestId)` count once; a sidechain copy of the same message under another request also collides. Prefer non-sidechain, then larger total tokens, then explicit speed, then first path/line. Distinct non-sidechain requests remain distinct. Different models on a colliding identity are excluded rather than assigned an invented model. Dedup precedes calendar filtering.

Require input/output nonnegative integer buckets; omitted cache read/write buckets mean zero as in the pinned Claude format, explicit null/invalid means missing. Cache creation split (5m/1h) replaces, never adds to, its aggregate; inconsistent split and aggregate makes tokens unknown. Unknown speed/service_tier or unsupported nested advisor iterations retain main-model tokens and mark partial.

## Token-only output

Daily and period keys, sorting, local calendar dates (today and preceding 29 dates), future exclusion and missing-day behavior are shared across local histories. Model rows expose event counts, optional decimal-string token totals, and missing-token counts. An incomplete measurement suppresses the aggregate token total. Partial status signals excluded or incomplete data. No monetary amounts are read, computed, or emitted. Provider-reported quota and balances remain separate.

No prompt/text, request/session IDs, source paths, credentials or content enter output.

## Bounds

Native capability-relative no-follow directory/file opens, regular-file admission and nonblocking flags reject symlinks/FIFOs. At most depth 12, 4,096 entries, 128 JSONL files, 4 MiB/file, 32 MiB total, 20,000 nonempty complete lines, 256 KiB/line, 10,000 accepted records, bucket <= 10^12. A two-second cooperative scan deadline is not a kernel-read hard timeout. Hard bounds/admission failures discard the scan as unavailable; malformed complete lines and partial trailing records are counted as excluded. No persistent cache, source writes or source SQLite access.

All new execution fixtures are synthetic documented-schema controls, not authentic user logs. Earlier immutable acceptance captures remain unchanged and do not certify these uncommitted changes.
