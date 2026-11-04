# Browser transport contract

## Purpose

The transport selected for a request is a truthful capability statement, not a
cosmetic profile name. Browser identity, transport fingerprint and browser
automation are separate concerns.

## Modes

| Mode | Meaning | Prohibited claim |
|---|---|---|
| `static` | The owned Rust HTTP transport for documents and APIs. | That it is Chrome, Safari, Firefox, or an impersonated browser. |
| `browser` | The requested installed local engine executes the request. Supported engines are `chrome`, `safari` and `firefox`. | That a different engine or generic HTTP stack was used. |
| `compat` (retired) | Formerly the pinned Go/AzureTLS legacy transport. No longer selectable; returns the typed `transport_retired` error. | Any claim that it is available. |

Mode and, for browser mode, engine selection are explicit. A request has exactly
one mode. A requested unavailable mode/engine returns a stable typed error; no
implementation may silently substitute `static`, Chrome, Safari or Firefox.

## Browser-mode acceptance rules

Each engine has a separate native gate. It must prove local discovery or
explicit attach, prerequisite diagnostics, navigation and redirects, JavaScript,
cookies and storage, response/download capture where supported, timeout, and
owned-process cleanup. Capability reporting is engine-specific: unimplemented
operations fail explicitly rather than returning fabricated partial results.

- **Chrome:** local Chrome/Chromium through its CDP-compatible adapter.
- **Safari:** macOS-only local Safari through Apple Events and/or
  SafariDriver/WebDriver BiDi. Remote Automation is a documented prerequisite,
  not a hidden setup action.
- **Firefox:** local Firefox through WebDriver/BiDi on macOS, Linux and Windows.

Browser mode relies on the real engine's version, network stack, cookies,
storage and JavaScript state. It is tested functionally against a hermetic
fixture server; it does not promise a frozen JA3/JA4 string.

## Compatibility mode (retired)

The `compat` transport (Go/AzureTLS sidecar, NDJSON IPC) was retired on
2026-11-03 by explicit decision. The Rust product no longer spawns or
references the Go sidecar, and the `symbrowse-compat` crate was removed.

Selecting `compat` — through `mode` in TOML, `SYMBROWSE_MODE`, or `--mode` —
fails with one stable typed error and never falls back to `static` or
`browser`:

| Surface | Result |
|---|---|
| `config show` / any config load (TOML, env, flag) | error `config`, exit code 9, message `invalid configuration: transport_retired: …`, `error.details.code = "transport_retired"` |
| `daemon --mode compat` | same envelope (`--json`) or one stderr line (text), exit code 9; no socket is bound |
| `mcp` | one stderr line, exit code 9; stdout stays clean |
| daemon session spec carrying `mode = "compat"` | daemon error code `transport_retired` (CLI maps it to `config`, exit 9) |

`engine = "static"` does not absorb `mode = "compat"`. Contract rows FETCH-002
and FETCH-011 are accepted differences; historical evidence
(`port/evidence`, `rust009-tls-results.json`,
`port/harness/cases/compat-sidecar.json`) is retained unmodified. The
`compat-sidecar` and `fetch-fingerprints` harness suites are retired.

## Cutover

Release approval requires all static and individual Chrome/Safari/
Firefox contract rows to pass on their declared platforms. Safari is only
advertised where its macOS gate passes. A mode is not globally advertised based
on another engine's result.

## Selection implementation (Phase 5A)

Rust resolves `SYMBROWSE_MODE`/`mode` and `SYMBROWSE_ENGINE`/`engine` with
flag-over-file precedence. The typed selection is shared by the CLI, daemon
and MCP autostart path. Browser mode requires one of `chrome`, `safari` or
`firefox`; an engine is rejected in static mode. Unknown, retired (`compat`) and
unavailable selections are typed failures, never substitutions. The
`version --json` handshake is unchanged; transport metadata is confined to
ordinary operation results.
