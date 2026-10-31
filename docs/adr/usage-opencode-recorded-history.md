# OpenCode recorded local history

This additive schema-1 capability reuses [LocalHistory](usage-grok-recorded-history.md), independently of quota/authentication results. It is not account billing.

```sh
SYMBRAIN_USAGE_OPENCODE_HISTORY=1 symbrain usage --providers opencode --output json
```

Only the exact flag value `1` **and explicit provider selection including `opencode`** enable scanning. The historical default graph, disabled/nonexact flags and other selections do not activate it. History has no constructor I/O. Quota discovery still uses its original account-selection, multichannel rejection and logout rules; history cannot repair/change a quota account or cause credential/network writes.

## Source and classification

All added fixtures are synthetic documented-schema controls, not user/native OpenCode captures.

Data directory: absolute `OPENCODE_DATA_DIR`, else `XDG_DATA_HOME/opencode`, else `$HOME/.local/share/opencode`. No tilde expansion; `.`/`..` or symlinked ancestors are rejected. Union path-sorted `opencode*.db`, including stable and preview. Read v1 `message(id,time_created,data)` and v2 `session_message(id,time_created,type,data)` where present. A database without either table contributes no rows; unreadable/schema-invalid database admission fails the scan closed.

- v1: `data.role == assistant`, flat `providerID` / `modelID`.
- v2: `type == assistant`, or `type == compaction` with `data.status == completed`; nested `model.providerID` / `model.id` (legacy flat rows copied into the v2 table are supported). Conflicting nested/flat identities are rejected.
- Only recorded provider IDs `opencode-go` (Go) and `opencode` (Zen) contribute. `openai` is excluded regardless of current OAuth/API login: it is never attributed to Go/Zen. This avoids reading credentials to distinguish Codex-owned ChatGPT OAuth from ordinary OpenAI API traffic. Other non-hosted provider IDs likewise contribute nothing. Missing/invalid ownership is a visible excluded diagnostic, never current-account attribution.
- Match upstream's assistant eligibility, including assistant rows without a separate completion marker; completed compactions are the only accepted compactions. Timestamp is integer `time_created` milliseconds, not inferred completion time.
- Deduplicate **message ID alone across all tables/channels**. Identical normalized provider/model/time/measurements count once; conflicting copies (including non-hosted/unknown ownership or model changes) do not count. A malformed identifiable copy poisons an otherwise eligible duplicate. Missing/oversized IDs are excluded, not independently counted. `excluded_events` is a diagnostic contribution/identity count, not exact lost messages.

## Unchanged consumer shape and measurement policy

`providers[].history` uses the existing required `scope`, `attribution`, `timezone`, `status`, `excluded_events`, `daily`, `periods`. Scope is always `local_machine`, attribution `unattributed`. Current credential/account identity is not evidence of historical ownership, even with only one channel. Multiple channels are unioned only under this explicit local/unattributed scope.

Daily keys ascend, model keys sort lexically; period keys are `last_30_days`, `today`, `yesterday`. The window is today plus the preceding 29 **host-local calendar dates**, yesterday is a calendar date, and future instants are excluded. Missing periods/days are not fabricated zero rows. `timezone: local` describes the Brain host, not a remote UI's zone.

Model rows retain required `model`, `events`, `missing_tokens`, and optional decimal-string `tokens`. Prefer exact nonnegative integer `tokens.total` when present; otherwise require all five documented buckets `input + output + reasoning + cache.read + cache.write`. Incomplete buckets are unknown, not zero. No additive use of buckets when total exists. Each bucket is bounded at 10^12; total at 10^15.

History ignores monetary values. Any missing token contribution suppresses that aggregate and marks history `partial`; rejected identities also mark it `partial`. No account-wide total is exposed. Malformed numeric JSON rejected by a runtime parser is an excluded row, not a fabricated value.

## Safety and limits

Both consumers share their existing private DB/WAL admission/copy helpers with credential discovery; the WAL validator is not duplicated. Directory capabilities/no-follow/nonblocking source opens, regular-file admission, repeated source-byte agreement, WAL integrity and SQLite integrity/query-only controls are preserved. SQLite opens **only private copies**; no source SQL, SHM creation, checkpoint or persistent history cache. The whole private directory is removed on normal/SQL-error return. No raw prompts, source paths or secrets enter history output.

Bounds: 4,096 directory entries (Rust also observes `.`/`..`), 8 databases; existing per-database 64 MiB DB / 16 MiB WAL / 1 MiB SHM limits, journal rejected. At most 10,000 source rows across tables/channels **before filtering or dedup**, 256 KiB JSON per row and 32 MiB total selected payload. Hard-limit/SQL/admission errors discard the scan (`unavailable`, empty arrays). The cooperative two-second history deadline is checked around bounded work and within SQL via context/progress cancellation. It is not a hard deadline for blocked kernel filesystem reads, directory enumeration or SQLite connection opening; no atomic cross-channel snapshot is claimed. No prices/history network calls or credential writes.

Native macOS synthetic CLI checks run with networking, real user-home/keychain reads and credential subprocess execution denied. WAL/error controls compare complete synthetic source inventories and empty private-copy directories. These establish implementation behavior, not real-user/account acceptance or all-platform certification. The accepted earlier immutable capture remains unchanged and valid only for its captured revision; its freshness gate is intentionally not green for these uncommitted changes.
