# Current Hermes Nous usage credentials

Usage reads `$HERMES_HOME/auth.json` (default `~/.hermes/auth.json`) without
refreshing, locking for mutation, or writing back. It accepts the historical
`providers: [{id: "nous", invoke_jwt, access_token}]` layout and current
`providers: {nous: {access_token, expires_at, last_auth_error, ...}}` layout.
When no Nous provider state exists, it can read `credential_pool.nous[]`.
An existing provider state is authoritative: a quarantined provider must not
fall through to another pooled account.

Current Hermes writes OAuth tokens directly to this private JSON store (see
`hermes_cli/auth.py` `_load_auth_store` / `_store_provider_state` and
`hermes_cli/auth_nous.py`); no Nous Keychain lookup is inferred. Portal usage
prefers the current state's `access_token`, not its inference-only `agent_key`.
The legacy array keeps its historical invoke-token preference.
`last_auth_error` presence blocks a stale token; its contents are never surfaced.
Expired or unusable stores report **Hermes Nous login missing/expired** in
`auth_status`, rather than an undifferentiated missing-credentials hint.
`NOUS_PORTAL_ACCESS_TOKEN` remains the explicit override.

## Immutable oracle divergence (#933)

The pinned Go revision and frozen fixtures are unchanged. The fresh Hermes
constructor replay and CLI comparator admit only the specific auth-status
replacement for an existing, unusable auth file. All other report fields,
authorization headers, store bytes, process exits and stderr must still agree.
CLI receipts distinguish `byte_matches` from `known_divergence`; this diagnostic
change is not claimed as byte parity. `scripts/usage-hermes-oracle/cli_test.py`
rejects unrelated changes. Current Go and Rust use the same new behavior;
current provider-map/pool fixtures are tested separately from the old oracle.

Tests use synthetic credentials. They do not log in, rotate tokens, read real
Keychain secrets, or establish the liveness of an operator's subscription.
