# Current Copilot CLI credential discovery (#932)

Both implementations read `~/.copilot/config.json` as JSONC (BOM, line/block
comments and trailing commas are accepted without changing string/URL bytes).
`lastLoggedInUser` selects an exact member of `loggedInUsers`. Without it, only
a single unambiguous logged-in user is accepted. Never choose an arbitrary
account, a token for another account, or a legacy token while a current account
is selected. A settings-only/current signed-out config does not hide legacy
`~/.config/github-copilot/{apps,hosts}.json` credentials.

The selected account key is `<host>:<login>`:

1. `COPILOT_ACCESS_TOKEN` remains the explicit override, including reference
   resolution failures (a failed explicit override must not fall through).
2. Current config plaintext fallback: `authTokens[account + ":github"].token`,
   then `copilotTokens[account]`.
3. On macOS, the exact Keychain item: service `copilot-cli`, account
   `https://github.com:<login>`. This reuses the bounded, timeout-protected
   security-helper pattern used for Claude. It can raise an OS approval panel.
4. Legacy apps/hosts files are retained when no current CLI account is present.

The current CLI source is deliberately limited to `https://github.com`.
Unsupported hosts, ambiguous selection, unreadable JSONC, denied/missing native
storage and missing tokens produce the specific **Copilot CLI login detected
but selected account token is unavailable** auth diagnostic. Native secure-store
reads on Linux/Windows are not implemented; plaintext current config and legacy
files work there. There is no login, refresh, store mutation, or `gh auth token`
fallback: using gh's default account could silently query a different user.

Public corroboration of the Keychain service/account layout:
<https://github.com/kunchenguid/quota-axi#provider-notes>.
Hermes' `hermes_cli/models.py` `_copilot_cli_config_tokens` independently reads
the Copilot CLI's `copilotTokens` plaintext fallback. Structure-only local
inspection also found `authTokens[account + ":github"].token`; no token values
were printed, copied into fixtures, or queried from a real Keychain.

`rust/symbrain-usage/tests/fixtures/current_copilot_cli.json` is a shared synthetic
11-case Go/Rust decoder corpus, not a replacement or repin of immutable Go
oracle evidence. Constructor tests isolate HOME and inject Keychain reads;
macOS helper tests use the existing fake `securityCommand`, never real secrets.
Historical apps/hosts and provider request/response tests remain in place.
