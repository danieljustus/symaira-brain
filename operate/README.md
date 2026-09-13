# Symaira Brain Operate module

> **Current ownership — source/consumer cutover completed 2026-09-13:** Operate is an optional Brain module at `symaira-brain/operate/`. Brain builds and manages the `symoperate` binary; the `symcockpit operate` source route was removed when Cockpit became tune-only. No signed Brain-built module release or package-manager replacement has shipped.

> Let an AI agent see and drive your Mac — locally, over MCP.

`symoperate` is a native macOS desktop-automation **MCP server**. It exposes
screenshots, the Accessibility tree, mouse/keyboard input, and app/window control
over stdio, so an agent can operate the GUI: open an app, find a button, click it,
type, save. It is a supervised, local tool — not a remote-control daemon.

## Managed install

From a verified Brain checkout, install the selected optional module:

```bash
symbrain setup --from-source /absolute/path/to/symaira-brain --modules operate --json
~/.symaira/bin/symoperate doctor
```

This creates a provenance sidecar beside the managed binary. Existing historical
Homebrew/Cockpit artifacts are not overwritten. The source package stays
independently buildable for development:

```bash
swift build --package-path operate
swift test --package-path operate
```

For user-facing module selection and profile exposure, use the Brain README.
The remaining release/signing and package-manager migration is a separate gate.

## Why symoperate?

- **Local and supervised.** No remote listener, no daemon. The agent sends one
  action at a time over stdio and gets fresh state back.
- **MCP-native.** Works with any MCP host — Claude Desktop, OpenCode, Cursor, …
  — without host-specific plugins.
- **Element-first.** Prefer stable accessibility `element_id`s over brittle
  screen coordinates; re-snapshot after each UI change.
- **Safety-guarded.** Refuses destructive controls and secure text fields; never
  automate passwords or permission dialogs without explicit confirmation.
- **Native macOS.** Built with AppKit, Accessibility, and ScreenCaptureKit for
  reliable performance on macOS 26+.

## Requirements

- macOS 26+
- `Accessibility` and `Screen Recording` permissions for the host process.
- A deliberate configuration/policy decision before exposing side-effecting MCP tools. The managed installation command above installs no automatic profile exposure and grants no permissions.

## CLI

```text
symoperate serve [--grant <permission>[,<permission>...]]
symoperate doctor                         Permission status + effective grant + environment probes (JSON)
symoperate version                        Print version and check for updates (JSON)
symoperate history --json                 Print the local operation history (JSON)
symoperate updates check [--force]        Check for updates and print result (JSON)
symoperate updates skip [<version>]       Show skipped version, or skip a specific version
symoperate updates clear-skip             Clear the skipped version
symoperate permissions status             Current macOS permissions
symoperate permissions grant accessibility
symoperate permissions grant screen
```

### Startup grant

`serve` accepts a startup permission ceiling. Repeat `--grant` or separate
permissions with commas, for example `symoperate serve --grant capture,input`.
When the flag is absent, `~/.config/symoperate/policy.json` is read; use an
object such as `{ "granted_permissions": ["capture", "input"] }`. The flag
wins over the file. When neither source is present, the historical full grant
is retained. The MCP `set_policy` tool may narrow this startup grant but cannot
widen it. `doctor` reports the resulting `effective_grant` array.

The grantable permissions are `capture`, `input`, `app_control`, `menu_action`
and `policy_modify`. Each one gates a group of actions, so withholding it
changes what the server will do.

Destructive controls and secure text fields are refused unconditionally and are
not permissions you grant. `destructive_action` and `secure_field_access` were
once listed as grantable and gated nothing; they are still accepted in a
`--grant` value or policy file so existing configurations keep starting, and
they grant nothing.

### Terminal demo

A real first-run of `symoperate doctor` on a machine where the Accessibility
permission is not yet granted — it reports exactly what is missing instead of
failing silently:

```console
$ symoperate doctor
{
  "capabilities" : {
    "accessibility" : false,
    "multi_display" : false,
    "ocr" : true,
    "screenshot" : true
  },
  "environment" : {
    "appsCount" : 8,
    "displaysCount" : 1,
    "macOSVersion" : "27.0.0",
    "platform" : "macOS",
    "swiftVersion" : "unknown"
  },
  "ok" : false,
  "permissions" : {
    "accessibilityGranted" : false,
    "screenRecordingGranted" : true,
    "source" : {
      "executablePath" : "/path/to/symoperate",
      "note" : "These booleans describe the TCC grants held by the process identified above. On macOS, TCC permissions (Accessibility, Screen Recording) are per-process — granting them to the launching app (e.g., Terminal, Cursor) does NOT make them available to the MCP host that launched symoperate. Grant permissions from the process that will actually be using symoperate's MCP server.",
      "pid" : 30337,
      "ppid" : 30303
    }
  },
  "recommendations" : [
    "Accessibility permission denied."
  ],
  "version" : "0.6.1"
}
```

Grant the missing permissions (`symoperate permissions grant accessibility`)
and re-run `doctor` — `ok` flips to `true` once every capability is available.
(`executablePath`/`pid` above reflect the machine the output was captured on.)

## MCP tools

`snapshot`, `query_ui`, `query_ui_ocr`, `find_ui`, `list_apps`, `list_windows`,
`list_displays`, `click`, `type_text`, `press_keys`, `scroll`, `drag`,
`launch_app`, `focus_window`, `menu_action`, `wait_for`, `permissions_status`,
`get_policy`, `set_policy`, `version`.

Register with an MCP host:

```json
{ "mcpServers": { "symoperate": { "command": "/abs/path/symoperate", "args": ["serve"] } } }
```

### Recommended agent loop

1. `query_ui` (or `snapshot`) → 2. decide → 3. prefer `element_id` over raw
coordinates → 4. one action → 5. re-snapshot before the next step.

## Safety

Supervised, local, stdio-only. Destructive controls and secure text fields are
refused for element-based actions. Do not automate passwords, payments or
permission dialogs without explicit user confirmation. Configure any exposure
through the Brain profile/tool allowlist; the source package itself is not an
authority grant.

## Documentation

- Brain root `README.md` — optional-module configuration, managed installation and profile exposure.
- `SOURCE_PROVENANCE.md` — source receipt and the completed ownership cutover.
- `docs/macos-agent-guide.md` — direct macOS registration, permission and safety guidance.

## License

Apache-2.0 © 2026 Daniel Justus.
