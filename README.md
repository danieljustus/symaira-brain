# symbrain

[![CI](https://github.com/danieljustus/symaira-brain/actions/workflows/ci.yml/badge.svg)](https://github.com/danieljustus/symaira-brain/actions/workflows/ci.yml)
[![License: Apache 2.0](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](LICENSE)
[![Go Version](https://img.shields.io/badge/Go-1.26%2B-00ADD8)](go.mod)

`symbrain` is the portable agent-context layer for AI coding harnesses. It
multiplexes the three Symaira *state cores* — `symvault` (credentials),
`symmemory` (memory/entities), and `symskills` (skill catalog) — behind one
MCP gateway, with one **profile** per harness connection controlling exactly
what that harness is allowed to see.

Point Claude Code, Cursor, Codex, Gemini, or opencode at `symbrain` once, and
every one of them talks to the same underlying vault, memory, and skills —
each through its own profile, each seeing only what that profile exposes.

> **Status:** `v0.1.0` released, in active development. The
> command reference below states clearly which subcommands are implemented
> today versus planned for a later milestone. Interfaces may still change
> before `v1.0.0`.

## What symbrain is not

- **Not a generic MCP hub or aggregator.** It only multiplexes the three
  state cores above. General-purpose tools (web fetch, browser automation,
  search, etc.) are wired directly into the harness by the user — symbrain
  does not proxy them.
- **Not a call-time policy enforcer.** See the boundary table below —
  that job belongs to [`symguard`](https://github.com/danieljustus/symaira-guard).
- **Not a memory store.** symbrain persists no memories and no secrets
  itself. It only holds profiles, the instructions source, and a local
  audit log.
- **Not a GUI**, at least not in `v0.1.0` (though native SwiftUI apps now
  exist in the repo — see [Native Apps](#native-apps)).

### symbrain vs. symguard

Both tools sit between a harness and its tool servers, but answer a
different question:

| | **symbrain** | **symguard** |
|---|---|---|
| Question | *What is this agent even allowed to see?* | *Is this specific call allowed to happen right now?* |
| Mechanism | Capability shaping: filtered `tools/list`, servers on/off per profile, modes like vault `request_only` | Conduct policing: risk classification, allow/ask/deny/redact per call, human approval, schema pinning, hash-chain audit |
| Scope | Only the Symaira state cores | Any MCP server, any client |
| Timing | At handshake / catalog build | On every tool call |
| Audit | Lightweight JSONL log (who/what/when, redacted) | Tamper-evident audit (hash chain) |

If you need per-call approval, risk classification, or a tamper-evident
audit trail, put `symguard` in front of your servers. symbrain does not
implement any of that itself.

## Quickstart

Install the latest release or build from source (see the
[Building](#building) section for the exact commands), then:

```bash
# 1. Create the XDG config/data/cache directories, a default config,
#    and two example profiles ("personal" and "restricted").
symbrain init

# 2. Register symbrain as an MCP server in Claude Code's config, bound
#    to the "personal" profile. --dry-run first if you want to preview
#    the exact change before it touches disk.
symbrain install --harness claude --profile personal --dry-run
symbrain install --harness claude --profile personal

# 3. Verify: symbrain doctor reports whether the environment, the state
#    core binaries, the profiles, and the harness registration all look
#    correct.
symbrain doctor
```

`symbrain doctor` after step 2 confirms the harness is wired up:

```text
✓  claude   installed, profile "personal": ~/.claude.json
```

Restart Claude Code (or reload its MCP connections) and the `symbrain`
server appears with the tools your profile exposes.

Supported `--harness` values: `claude`, `claude-desktop`, `cursor`,
`opencode`, `codex`, `gemini`. `symbrain uninstall --harness <name>` reverses
step 2 and only ever touches the `symbrain` entry — every other server in
that harness's config is left alone.

## Profile guide

A profile is a TOML file under `~/.config/symbrain/profiles/<name>.toml`
that controls, per state core, whether it's exposed at all and — for vault
and memory — which named **mode** shapes the tool list. `symbrain init`
writes two starting points:

**`personal`** — full access for a trusted, single-user setup:

```toml
[servers.vault]
enabled = true
mode    = "full"

[servers.memory]
enabled = true
mode    = "read_write"

[servers.skills]
enabled = true
```

**`restricted`** — least-privilege, for an untrusted or shared harness
connection:

```toml
[servers.vault]
enabled = true
mode    = "request_only"

[servers.memory]
enabled = true
mode    = "read_only"

[servers.skills]
enabled = true
```

The mode is what makes the difference concrete. Run `symbrain profile show
restricted` and the tool list it prints backs up the claim: **a harness
bound to `restricted` can search memory but can never read a secret
directly.**

```text
vault: enabled=true mode=request_only
  effective exposed: generate_password, health, request_credential
  effective hidden:  find_entries, get_entry, get_entry_metadata,
                      set_entry_field, symaira_audit_self, symaira_search,
                      symaira_whoami

memory: enabled=true mode=read_only
  effective exposed: entity_list, entity_resolve, graph_neighbors,
                      memory_get, memory_list, memory_search
  effective hidden:  entity_relate, memory_set
```

`request_only` mode hides every tool that returns a secret value
(`get_entry`, `find_entries`, ...) and exposes only `request_credential` —
the flow where the *user's own* password manager UI supplies the credential
directly to the caller, never through the harness. `read_only` mode on
memory hides `memory_set`/`entity_relate` so a restricted harness can look
things up but never write.

Manage profiles with:

```bash
symbrain profile list                          # every profile + servers summary
symbrain profile show <name> [--json]           # full detail incl. effective tool list
symbrain profile add <name> [--from personal|restricted]
symbrain profile remove <name> [--force]
```

## Command reference

Implemented today:

| Command | Purpose |
|---|---|
| `symbrain init` | Create XDG directories, default `config.toml`, and example profiles |
| `symbrain doctor [--json]` | Check environment, config, state-core binaries, profiles, and harness registrations |
| `symbrain profile list \| show \| add \| remove` | Manage profiles under `~/.config/symbrain/profiles/` |
| `symbrain install --harness <name> --profile <name> [--project DIR] [--dry-run]` | Register symbrain as an MCP server in a harness's config |
| `symbrain uninstall --harness <name> [--project DIR] [--dry-run]` | Remove symbrain's entry from a harness's config (only that entry) |
| `symbrain version [--json]` | Print version, Go runtime, and OS/arch |

Planned, not yet implemented (each prints a "not yet implemented" notice
naming its target milestone rather than failing silently):

| Command | Purpose |
|---|---|
| `symbrain serve --profile <name>` | Run the MCP gateway over stdio: merges the vault/memory/skills catalog per the bound profile and routes `tools/call` to the right child |
| `symbrain sync` | Push the canonical instructions/skills source out to installed harnesses |
| `symbrain audit` | Inspect the local JSONL audit log |

`install`/`uninstall` already write a working MCP entry that *points at*
`symbrain serve --profile <name>` — that entry only becomes live once
`serve` itself lands.

## Security notes

symbrain's job is **least exposure**, not call-time hardening — see the
boundary table above. Concretely:

- **What it protects against:** an over-broad harness connection seeing or
  using tools it has no business touching. A `restricted` profile's vault
  mode never exposes a tool that returns raw secret material — the harness
  literally cannot call `get_entry`, because that tool is absent from its
  `tools/list`, not merely discouraged.
- **What it does not protect against:** a malicious or compromised harness
  process abusing the tools its profile *does* expose (e.g. a `personal`
  profile with `vault` in `full` mode). There is no per-call approval, no
  risk scoring, and no human-in-the-loop confirmation in symbrain itself —
  that is `symguard`'s job.
- **Audit log:** when enabled (default: on), every routed tool call is
  recorded as JSONL under `~/.local/share/symbrain/audit/<profile>.jsonl`
  with who/what/when. Vault call arguments and results are never written to
  the audit log or to any error string, regardless of the `verbose` audit
  setting — `verbose` only adds non-vault tool arguments to the record.
- **Config files on disk:** profiles and the global config are written with
  `0o600` permissions; XDG directories are created `0o700`. Harness config
  files are backed up before symbrain edits them (`symbrain install` /
  `uninstall`).
- **Standalone by design:** symbrain never compiles against sibling repos.
  Child state-core binaries are located at runtime via `PATH` lookup with a
  timeout; a missing one is a `doctor` warning, never a hard failure.

Found a security issue? Please report it privately rather than opening a
public issue — see [SECURITY.md](.github/SECURITY.md) for how to report it.

## XDG paths

| Purpose | Path | Overridable via |
|---|---|---|
| Config (`config.toml`, profiles) | `~/.config/symbrain/` | — |
| Data (audit log) | `~/.local/share/symbrain/` | `$XDG_DATA_HOME` |
| Cache | `~/.cache/symbrain/` | `$XDG_CACHE_HOME` |

Config resolution intentionally does not consult `$XDG_CONFIG_HOME` (it
reuses `corekit/configkit`'s fixed path resolution), so a profile written by
`symbrain init` is always the exact file later commands read back.

## Building

Download the archive for your platform from the [latest GitHub
Release](https://github.com/danieljustus/symaira-brain/releases/latest),
extract it, and place `symbrain` on your `PATH`.

Or build from source:

```bash
git clone https://github.com/danieljustus/symaira-brain.git
cd symaira-brain
go build -o symbrain ./cmd/symbrain
./symbrain version
```

Requirements: Go 1.26+, `CGO_ENABLED=0` (the release build is CGO-free; see
`make build`).

### Development

```bash
make build       # CGO_ENABLED=0 go build -o symbrain ./cmd/symbrain
make test        # go test ./...
make test-race   # go test -race ./...
make lint        # golangci-lint if available, else go vet
make fmt         # gofmt -w -s .

# Full local check (mirrors CI):
go vet ./... && go test -race ./... && go build -o symbrain ./cmd/symbrain
```

See [AGENTS.md](AGENTS.md) for coding conventions, package layout, and the
full architectural boundary rules referenced above.

## Native Apps

Native SwiftUI apps for macOS and iOS are included in the repo. They use
[`symaira-appkit`](https://github.com/danieljustus/symaira-appkit) for
theme, CLI runner, and tool detection.

### Build from source

```bash
brew install xcodegen   # if not already installed
xcodegen generate
xcodebuild build -project SymBrain.xcodeproj -scheme SymBrain -destination 'platform=macOS'
```

### Open in Xcode

```bash
xcodegen generate
open SymBrain.xcodeproj
```

**SymBrain** (macOS) is a full dashboard: doctor, profiles, harnesses, audit
log, and settings. **SymBrainMobile** (iOS) is a read-only companion showing
the state-core overview, tool registry, and setup guide.

## License

Apache License 2.0 — see [LICENSE](LICENSE).
