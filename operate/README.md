# operate — the `symcockpit operate` family

> **Accepted ownership transition, not removal:** [PB-2026-09-09](../docs/product-boundaries.md) retains Operate as an optional Brain module. The existing Cockpit entrypoint remains supported until verified migration. No Cua or Hermes replacement is mandated.

> Let an AI agent see and drive your Mac — locally, over MCP.

[![CI](https://github.com/danieljustus/symaira-cockpit/actions/workflows/ci.yml/badge.svg)](https://github.com/danieljustus/symaira-cockpit/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/danieljustus/symaira-cockpit?sort=semver)](https://github.com/danieljustus/symaira-cockpit/releases/latest)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)
[![Swift](https://img.shields.io/badge/Swift-6.0-orange.svg)](https://swift.org)

![Symaira Operate social preview](docs/assets/social-preview.png)

> This package was the standalone `symaira-operate` repository until
> 2026-08-23. It now ships as the `operate` family of
> [`symcockpit`](../README.md); the repo is archived and the `symoperate`
> cask is deprecated.

`symoperate` is a native macOS desktop-automation **MCP server**. It exposes
screenshots, the Accessibility tree, mouse/keyboard input, and app/window control
over stdio, so an agent (Claude Desktop, OpenCode, Cursor, …) can operate the
GUI: open an app, find a button, click it, type, save. It is a supervised, local
tool — not a remote-control daemon.

Part of the [Symaira](../README.md) family, and the sibling family of
[`tune`](../tune) (hardware tuning): **operate = GUI actions, tune =
thermals/brightness/power.**

> **Status: shipping** (`symcockpit version` reports this build's component
> versions). Working native implementation (rebranded from the author's
> `mac-operator` prototype), covered by unit tests across every target
> (`swift test`).

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

## Install

**Homebrew (recommended):**

```bash
brew install danieljustus/tap/symcockpit
```

**Direct download:** grab the latest `symoperate.dmg` from the
[Releases page](https://github.com/danieljustus/symaira-cockpit/releases/latest),
open it, and move `symoperate` to `/usr/local/bin/` (or any directory on your
`PATH`).

Then grant permissions and verify the install:

```bash
symoperate permissions grant accessibility
symoperate permissions grant screen
symoperate doctor
```

## Requirements

- macOS 26+
- `Accessibility` and `Screen Recording` permissions for the host process

## Build

```bash
swift build            # binary at .build/debug/symoperate
swift test             # run the test suite
swift run -q symoperate doctor
```

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

Supervised, local, stdio-only. Destructive controls (Delete/Trash/Uninstall/
Allow/Authorize/Unlock/Quit/…) and secure text fields are refused for
element-based actions. Don't automate passwords, payments, or permission dialogs
without explicit user confirmation. See [AGENTS.md](AGENTS.md), [SAFETY_AUDIT.md](SAFETY_AUDIT.md), and `NOTICE`.

## Documentation

- [docs/architecture.md](docs/architecture.md) — components & tool contract
- [docs/roadmap.md](docs/roadmap.md) — built vs planned

## License

Apache-2.0 © 2026 Daniel Justus.
