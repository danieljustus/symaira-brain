# Grok weekly quota (Brain #984)

Both Brain runtimes offer `grok` / `Grok` through explicit usage selection
(`symbrain usage --providers grok`). Historical defaults remain unchanged;
MCP can expose it only when its host injects an explicitly selected service.
No new dependencies or report-schema fields are introduced.

## Credentials and read-only boundary

- `GROK_ACCESS_TOKEN` wins over files, including when resolving its secret
  reference fails. The shared resolver supports `symvault://` references;
  Cockpit should supply an opaque reference, never read the secret itself.
- Otherwise read `$GROK_HOME/auth.json`, falling back to `~/.grok/auth.json`.
  Use the existing bounded, regular-file, no-final-symlink credential reader.
- The file is an account map: each entry's `key` is the access token. Trim outer
  whitespace, reject embedded control characters, and use `expires_at`, then
  the historical `expires`, plus a decodable JWT `exp` as expiry hints.
  An unverified JWT expiry hint is not authentication.
- Multiple distinct account tokens are intentionally unavailable: select one
  with `GROK_ACCESS_TOKEN` rather than silently showing another account's pool.
- No refresh endpoint, token rotation, file lock, writeback, keychain discovery,
  or refresh-token read is added. Expired credentials and HTTP 401/403 carry
  an actionable `grok login` instruction. Run the CLI's own authorized login
  or refresh flow outside the read-only usage report.

## Transport and mapping

Send only `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits`, with
`Authorization: Bearer …`, `X-XAI-Token-Auth: xai-grok-cli`, JSON `Accept`, and
Brain's own `User-Agent: symbrain`. Existing request/response limits, HTTPS
validation, timeout/cancellation and redirect refusal remain in force.

Read `config.currentPeriod.{type,start,end}` and `config.creditUsagePercent`.
Require valid forward timestamps and `USAGE_PERIOD_TYPE_WEEKLY`. Emit one
`Weekly limit` percent meter with limit `100` and the reported end as reset.
Clamp finite numeric percent values to 0–100. Numeric strings are accepted;
null, booleans, malformed or nonfinite numbers are rejected. An absent usage
field remains absent in the report; an explicit zero stays zero. This is a
conservative reporting choice even though upstream interprets proto-JSON
omission as zero. Monthly/legacy data never becomes a weekly meter.

A JSON HTTP 412 error containing `no personal team` reports unavailable
personal weekly quota without inventing values. Other 412s remain HTTP
failures. HTTP 429 keeps rate-limit classification and Retry-After handling.
Response bodies and transport errors are not copied into Grok diagnostics.

## Deliberate gaps

Schema v1 has no plan, badge, PAYG enabled flag or numeric PAYG cap metadata.
Do not represent `onDemandCap.val` as consumed credits, balance or spend.
PAYG status/cap and the optional settings plan request are deliberately not
exposed pending an agreed additive metadata schema. No schema change is made
here. No local history/cost scanner is added and no costs are inferred.

Protocol reference (read-only, not executed): OpenUsage commit
`abf472d5dbb23645c3afbb5a0c7429606ffbdcd7`,
`Providers/Grok/{GrokAuthStore,GrokUsageClient,GrokUsageMapper,GrokCreditsConfigDecoder}.swift`.
The implementation is independent and the tests use synthetic credentials,
fixture transports and isolated file stores, not live provider calls.
