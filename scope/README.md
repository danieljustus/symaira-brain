# Symaira Brain Scope module

> **Current ownership — source/consumer cutover completed 2026-09-13:** Scope is an independently optional Brain module at `symaira-brain/scope/`. Brain builds and manages the `symscope` binary; the `symcockpit scope` source route was removed when Cockpit became tune-only. No signed Brain-built module release or package-manager replacement has shipped. Scope-disabled Brain starts no probes or watchers; Scope requires neither Operate nor Browse nor a Memory/agent profile.

Port inventory and MCP discovery: **ports** (lsof), **containers** (Docker CLI), **background services** (launchd/Homebrew) and **MCP servers** (AI-client configurations), exposed via CLI and a stdio MCP server.

## Managed install

```bash
symbrain setup --from-source /absolute/path/to/symaira-brain --modules scope --json
~/.symaira/bin/symscope doctor
```

The source package remains independently buildable for development:

```bash
swift build --package-path scope
swift test --package-path scope
```

The local managed binary gets a provenance sidecar. Existing historical release artifacts are not overwritten; signed release/package-manager migration remains a separate gate.

## CLI

```bash
symscope version               # VersionInfo (--json für Machine-Output, schema_version 1)
symscope scan                  # aggregierter Snapshot (Ports + MCP + Container)
symscope ports list            # lauschende TCP/UDP-Ports mit Prozess
symscope ports suggest [n]     # n freie TCP-Ports vorschlagen (Default 3)
symscope mcp list              # MCP-Server über AI-Client-Configs
symscope mcp health            # Health-Probe aller konfigurierten Server
symscope daemons list          # launchd Agents/Daemons und Homebrew-Services
symscope daemons list --all    # inklusive com.apple.* launchd-Services
symscope daemons health        # Zustands-/Exit-Status-Zusammenfassung
symscope containers            # laufende Docker-Container (docker ps)
symscope conflicts             # Ports, die von mehreren Prozessen gehalten werden
symscope watch --interval 5    # Änderungen beobachten (NDJSON-Events: port_bound, …)
symscope cache show|clear      # Snapshot-Cache inspizieren/löschen
symscope explain port <p>      # was nutzt Port p (Prozesse + MCP-Server)
symscope explain server <name> # welcher Client/Config gehört zum MCP-Server
symscope serve                 # stdio MCP-Server (JSON-RPC)
```

## Module layout

```
symscope (executable) → SymScopeMCP → SymScopeCore
```

- `SymScopeCore` — PortService (lsof), MCPDiscovery, ContainerService (docker), DaemonService (launchctl/plists/Homebrew), MCPHealthService, ConflictDetector, SnapshotService and models. No external dependency beyond Foundation and Darwin.
- `SymScopeMCP` — stdio JSON-RPC/MCP via exact-pinned Symaira AppKit; seven tools: `scan`, `ports_list`, `ports_suggest`, `mcp_list`, `conflicts`, `mcp_health`, `daemons_list`.
- `symscope` — the thin CLI/stdio-MCP entrypoint.

## Conventions

- JSON uses `snake_case`; `serve` writes structured JSON-RPC frames only to stdout.
- Exit codes: `0` success, `1` runtime error, `2` CLI usage error.
- Discovery is local and read-only. The only network interaction is an explicitly requested `mcp health` probe.
- The binary is `symscope`; its source stays module-owned in Brain. See the Brain root README for optional-module configuration and managed consumer discovery.

## License

Apache-2.0 © 2026 Daniel Justus.
