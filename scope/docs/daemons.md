# Background-service inventory

`scope daemons list` is a read-only inventory of launchd agents/daemons and
Homebrew services. `--all` includes Apple-owned `com.apple.*` labels; the
default output keeps those rows out of the way so failures in user services
surface first. Rows with a non-zero `last_exit_status` sort before healthy rows.

The inventory joins three local sources:

- `launchctl list` supplies label, PID and last exit status for user and system
  domains.
- Plists in `~/Library/LaunchAgents`, `/Library/LaunchAgents` and
  `/Library/LaunchDaemons` supply `keep_alive`, `run_at_load`, domain and the
  manual/brew origin. A malformed plist becomes a note and does not abort the
  scan.
- `brew services list` supplies Homebrew provenance. Homebrew is optional; when
  `brew` is absent the result remains successful and carries a note.

Listening ports are joined by PID, so a daemon row carries its port numbers and
conflict holders can include the launchd label. The same model is included in
`scope scan` and the `daemons_list` MCP tool.

All process and filesystem access is behind `DaemonCommandRunning` and
`DaemonFileSystemReading`. Tests inject fixture command output and plist bytes;
no test invokes the host's `launchctl` or Homebrew installation.
