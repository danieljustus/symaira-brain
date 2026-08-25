# Memory Sync Migration — `symmemory` → `symbrain memory sync`

Status: **proposal / migration guide** (local planning doc, not part of the module).
Issue: danieljustus/symaira-brain#301.

The archived `symmemory` runtime (repo `symaira-memory`, Homebrew formula
`danieljustus/tap/symmemory`, last release 0.17.0) provided remote memory
synchronization through `symmemory sync`. That runtime is archived; the
supported replacement is now in-process in `symbrain`:

```
symbrain memory sync --remote <url> [--pull] [--push] [--token <t>]
                     [--encrypted-relay --relay-passphrase <p>]
```

It speaks the **same HTTP sync API** the old runtime used
(`/api/sync/changes`, `/api/sync/apply`, `/api/sync/relay` on the memory
server), so an existing remote server keeps working unchanged. The local
database is reused in place — same XDG paths (`~/.local/share/symmemory/…`
and `~/.config/symmemory/…`), same per-remote sync cursors
(`db.GetSyncCursor`/`SetSyncCursor`), same oplog/tombstone LWW conflict
model. There is no export/import step and no shell-out to a `symmemory`
binary.

## Flag mapping (legacy → new)

| Legacy `symmemory sync` | New `symbrain memory sync` | Implemented |
|---|---|---|
| `--remote <url>` | `--remote <url>` (required) | ✅ in-process |
| `--pull-only` | `--pull` | ✅ in-process |
| `--push-only` | `--push` | ✅ in-process |
| (default: pull + push) | (default: pull + push when neither flag given) | ✅ |
| `--token <t>` | `--token <t>` or `$SYMBRAIN_MEMORY_SYNC_TOKEN` | ✅ in-process |
| `--relay` | `--encrypted-relay` | ✅ in-process |
| `--passphrase-file <file>` | `--relay-passphrase <p>` or `$SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE` | ✅ (read the file yourself: `--relay-passphrase "$(cat <file>)"`, or export the env var) |
| `-o json` | `--output json` / `--json` | ✅ |
| `--remote` pointing at a tunneled `http://localhost:8787` | identical URL works | ✅ |

All four legacy modes (pull-only, push-only, token auth, encrypted relay)
are implemented in-process, so **no legacy mode needs a documented
replacement** — the mapping table above is the only documentation required.

Security notes (placeholders, not secrets):
- Never put the token or passphrase on a command line visible to other
  processes; prefer `$SYMBRAIN_MEMORY_SYNC_TOKEN` and
  `$SYMBRAIN_MEMORY_SYNC_RELAY_PASSPHRASE` (the LaunchAgent plists below use
  environment variables).
- With `--encrypted-relay` the remote server stores AES-256-GCM ciphertext
  only; memory content never leaves a peer in plaintext. Both peers must
  share the same passphrase.
- The token/passphrase values are never logged or echoed by the command.

## Current state of the old setup (this machine, 2026-08-25)

- Homebrew formula `danieljustus/tap/symmemory` **0.17.0 installed**
  (formula is disabled/deprecated upstream).
- LaunchAgent `~/Library/LaunchAgents/com.daniel.symmemory-tunnel.plist`
  **loaded** (label `com.daniel.symmemory-tunnel`): `ssh -N -L
  8787:localhost:8787 macmini`, RunAtLoad + KeepAlive, log to
  `~/Library/Logs/symmemory-tunnel.err.log`.
- The six-hour sync cadence was provided by the old runtime's scheduler
  (it ran `symmemory sync` against the tunneled remote every 6 h). No
  separate `symmemory-sync` plist exists on this machine; if one exists on
  the remote side, replace it with the LaunchAgent below.

## Migration

### 1. Install the new job (replaces the six-hour sync)

Write `~/Library/LaunchAgents/com.daniel.symbrain-memory-sync.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.daniel.symbrain-memory-sync</string>
    <key>ProgramArguments</key>
    <array>
        <string>/opt/homebrew/bin/symbrain</string>
        <string>memory</string>
        <string>sync</string>
        <string>--remote</string>
        <string>http://127.0.0.1:8787</string>
        <string>--output</string>
        <string>json</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>SYMBRAIN_MEMORY_SYNC_TOKEN</key>
        <string>REPLACE_WITH_REMOTE_API_TOKEN</string>
    </dict>
    <key>StartInterval</key>
    <integer>21600</integer>
    <key>RunAtLoad</key>
    <true/>
    <key>StandardErrorPath</key>
    <string>/Users/daniel/Library/Logs/symbrain-memory-sync.err.log</string>
    <key>StandardOutPath</key>
    <string>/Users/daniel/Library/Logs/symbrain-memory-sync.out.log</string>
</dict>
</plist>
```

Load it:

```sh
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.daniel.symbrain-memory-sync.plist 2>/dev/null || true
launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.daniel.symbrain-memory-sync.plist
launchctl kickstart -k gui/$(id -u)/com.daniel.symbrain-memory-sync   # run once immediately
```

Verify one run by hand first:

```sh
symbrain memory sync --remote http://127.0.0.1:8787 \
  --token "$SYMBRAIN_MEMORY_SYNC_TOKEN" --output json
```

Then check the logs: `tail -f ~/Library/Logs/symbrain-memory-sync.{out,err}.log`.
The JSON output contains `"cursor"` — it must advance between runs.

Note: the six-hour cadence is `StartInterval 21600` (every 6 h), matching
the old scheduler.

### 2. The SSH tunnel stays as-is during migration

`com.daniel.symmemory-tunnel` (ssh `-N -L 8787:localhost:8787 macmini`) is
**not** part of the retired runtime — it is the transport to the remote
Mac mini and must keep running. The sync job above targets the tunneled
`http://127.0.0.1:8787` exactly like the old job did.

### 3. When the old formula may be removed

Remove `symmemory` only after **both** conditions hold:

1. At least one successful `symbrain memory sync` run against the remote
   (cursor advanced, `"pulled"`/`"pushed"` counters as expected), and
2. The `com.daniel.symbrain-memory-sync` LaunchAgent survived at least two
   StartInterval fires with `cursor` advancing (one full 6 h cycle), so
   the scheduler migration is proven.

```sh
brew uninstall symmemory
```

Keep the tunnel agent (`com.daniel.symmemory-tunnel`) — nothing else
depends on the formula, but the tunnel is still the transport. If the
tunnel ever needs to be owned by the new stack instead, fold the ssh
forward into the sync job: add `/usr/bin/ssh -N -L 8787:localhost:8787
macmini` as a second ProgramArgument set, or wrap both in a small script.
That is optional; keeping the existing tunnel agent is the supported path.

### 4. Rollback

To go back to the old scheduler (only possible while the formula is still
installed):

```sh
# 1. Stop the new job
launchctl bootout gui/$(id -u) ~/Library/LaunchAgents/com.daniel.symbrain-memory-sync.plist
rm ~/Library/LaunchAgents/com.daniel.symbrain-memory-sync.plist

# 2. Restore whatever carried the six-hour job before (re-install the old
#    plist / re-enable the old scheduler), e.g.:
# launchctl bootstrap gui/$(id -u) ~/Library/LaunchAgents/com.daniel.symmemory-sync.plist

# 3. Verify with the old binary (formula must still be installed):
# symmemory sync --remote http://127.0.0.1:8787 --token <t>
```

Rollback is safe at any time: the new command never changes the sync
conflict model or the database schema beyond the sync state the old
runtime already used, and both binaries share the same database and
cursors — switching back and forth does not require a re-export.

## Notes

- `docs/` is git-ignored by design in this repository ("Local working docs —
  NOT published"); this guide is the local record for the migration. No
  tracked documentation presents the old runtime as required anymore.
- Related tracked docs (BETA-CHECKLIST.md) describe a historical 2026-07-21
  test run that legitimately used the binaries on PATH; it is a dated audit
  log, not guidance, and was left unchanged.
