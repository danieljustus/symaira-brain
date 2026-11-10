# Account-safe attribution and organization-billed Copilot (Brain #984 slices #1000, #1001)

Only the native Rust path (`rust/symbrain-usage`) applies; Go was removed in
#1003. Both additions are optional report fields that are absent unless there
is something to report, so the sealed oracle reports are unchanged.

## #1000 — accounts behind a Claude or Codex row

Verified read-only on macOS (key and file names only, no values):

| Source | Layout | Used for |
|---|---|---|
| Claude Code CLI | `~/.claude.json` → `oauthAccount.accountUuid`; token in Keychain service `Claude Code-credentials` (default login) and `Claude Code-credentials-<hex>` (another login) or `~/.claude/.credentials.json` | default login id; suffixed items are separate logins whose owner id is not on disk |
| Claude Desktop | `~/Library/Application Support/Claude/config.json` → `lastKnownAccountUuid`; tokens encrypted with Keychain item `Claude Safe Storage` | account id only; tokens are never read |
| Codex CLI | `$CODEX_HOME` (default `~/.codex`) `/auth.json` → `tokens.account_id`, else the `chatgpt_account_id` claim of `tokens.id_token`; there is no accounts directory | account id of the file login |

Rules:

- `ProviderUsage.accounts` lists `{label, source, state, detail?}` and is
  emitted **only** when more than one login is discovered, or the credential
  in use cannot be tied to one. A single clear login keeps the frozen shape.
- Label: `claude-<8 hex of SHA-256(account uuid)>`, `codex-<8 hex>`, or
  `claude-keychain-<suffix>`. No token, e-mail address or raw id is emitted.
- States: `used` (the only account whose usage the row reads — accounts are
  never merged), `not_read` (discovered, not read), `unattributed` (label
  `unattributed`; the credential's owner is unknown) and `candidate` (may own
  the unattributed credential).
- Unattributed when the token comes from an environment variable or vault
  reference, an Admin API key is configured (it is tried first), `CLAUDE_CONFIG_DIR`
  selects a login this reader does not read, or the Keychain item cannot be
  identified.
- Claude Desktop is discovered but never read: its tokens are encrypted and
  Brain does not touch Safe Storage.
- Nothing is written to any CLI or Desktop store.

Known limits:

- **Single Codex home (maintainer decision 2026-11-10).** Only the active
  `CODEX_HOME` and, when it is unset, the default `~/.codex` are inspected.
  Additional Codex logins are deliberately not discovered: there is no
  multi-home or multi-account scan, no accounts directory exists, and no
  layout with several Codex logins was available to record. A second login
  therefore never appears in `accounts`; point `CODEX_HOME` at it to report it.
- The Keychain listing (`security dump-keychain`, attributes only) runs only
  when the Claude token came from the Keychain or a suffixed
  `Claude Code-credentials-<hex>` item is looked for. A real dump is about
  550 KB; the output is read in full up to 16 MiB (20 s timeout). A larger
  dump is reported as truncated and ignored as a whole, never parsed
  partially, so no suffixed login is guessed from a cut-off listing.

## #1001 — Copilot seats billed to an organization

A seat with `token_based_billing: true` and no per-seat quota has no usage in
`/copilot_internal/user`. Only then, with the same credential and no new
scope, Brain calls `GET /user/orgs?per_page=100` and, for up to 10
organizations, `GET /orgs/{org}/settings/billing/usage/summary`. The
first summary with Copilot credit items (`product` Copilot, `unitType`
`ai-units` or `ai-credits`) yields one meter `Organization credits` (sum of
`grossQuantity`, an organization-wide month-to-date total, not seat usage).
No amount or price is read. Enterprise hosts use `https://<host>/api/v3`.

`snapshot.org_billing = {state, detail, organization?}` reports the outcome;
it never fails the row:

| state | meaning |
|---|---|
| `available` | meter shown; `organization` names the source |
| `forbidden` | HTTP 403/401: caller is not an organization owner or billing manager (or the token cannot list organizations) |
| `no_organization` | `/user/orgs` is empty |
| `no_copilot_usage` | no summary carried Copilot credit items |
| `unavailable` | transport error, 429 or 5xx |

The `forbidden` text is: "Organization billing is readable only by
organization owners and billing managers (HTTP 403); no organization usage is
shown". Personal seats never trigger the lookup.

Evidence note: no recorded organization response was available (the account
belongs to no organization), so the offline fixtures follow GitHub's
documented schema and the OpenUsage mapper at `abf472d`.
