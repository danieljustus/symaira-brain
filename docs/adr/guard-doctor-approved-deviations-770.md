# Guard doctor: native rendering of the remaining diagnostic states

Status: accepted for #770 (maintainer decision, 2026-10-19). Supersedes the
fail-closed clause in [ADR 0011](0011-standalone-rust-guard.md) and the
"delegated" rows of the
[boundary inventory](../../migration/guard-doctor-boundary-inventory-770.md).

## Context

`symguard doctor` stopped with `unsupported native diagnostic state` (exit 1,
no report) for states whose exact Go bytes the port could not reproduce: I/O
errors other than "missing", TOML syntax errors other than missing `=`, wrongly
typed known fields, case-insensitive aliases of known fields, several invalid
`defaults.*` values, anchor strings the typed decoder could not classify,
discovery parse errors, servers with neither `command` nor `url`, and Windows
paths that are not valid UTF-16. Some of these have no stable Go output at all:
Go picks the reported invalid default by map-range order, and its decoder and
syscall wording comes from libraries this port does not use.

## Decision

Every doctor state is rendered natively; there is no Go fallback or refusal.
Each state keeps Go's report row, error frame and exit code. Only the detail
text after the frame may differ, and it is deterministic:

| State | Native output |
|---|---|
| Config, audit log, anchor or discovery I/O error | Go's frame (`config: parse <path>: open <path>:`, `stat <path>:`, `anchor <path>: open <path>:`, `discovery: <client> (<path>): [unsupported] read failed: open <path>:`) followed by the platform error text without Rust's ` (os error N)` suffix |
| TOML syntax error other than missing `=` | `config: parse <path>: toml: line N: <toml_edit message>` |
| Known field with a wrong type | `config: parse <path>: toml: a known field has an unsupported type or a case-insensitive alias` |
| Several invalid `defaults.*` values | The first invalid key in key order instead of Go's random choice |
| Anchor string the typed decoder cannot classify | `auditkit: parse anchor: json: unsupported string encoding in anchor` |
| Discovery parse error | `discovery: <client> (<path>): [unsupported] <native JSON/YAML parser message>` |
| Server with neither `command` nor `url` | `discovery: <client> (<path>): [unsupported] server "<name>" is missing both command and url` (same text as Go) |
| Windows path that is not valid UTF-16 | Rendered through the existing `GoText` projection |

One deviation is deliberately **stricter** than Go: a case-insensitive alias of
a known field (`ProXy`, `Upstream`, `ſerver`) is accepted by Go's TOML decoder
with undocumented precedence rules. Doctor reports it as the decoder error above
(exit 1, policy not loaded) instead of guessing which binding Go would use. A
security configuration that silently binds a differently spelled key is worse
than an explicit error.

The earlier deliberate deviation (#640, sorted plaintext secret keys) is unchanged.

## Verification

`scripts/guard-standalone-oracle` compares every previously refused case with
the frozen Go handlers. Two shared contracts replace the refusal assertion:

- `approved-deviation`: exit code, stderr and file state are identical to Go;
  stdout is identical line for line except error rows, and each differing row
  must share Go's row label and at least three `: `-separated frame tokens.
- `approved-stricter` (aliases only): Go exits 0, native exits 1 with the fixed
  decoder message, an empty stderr and `policy not loaded`.

The previous refusal mutant is kept as a control and must now be rejected.
`rust/symguard-cli/tests/standalone.rs` covers syntax errors, several invalid
defaults (deterministic across runs), an audit `stat` error and a server without
`command`/`url` through the real binary. Native Linux, macOS and Windows CI must
still run the full gate before #770 is complete.
