# Symaira Brain macOS agent guide

> **Current source/consumer ownership (2026-09-13):** Operate and Scope are optional Brain modules. Hardware/system tuning remains the separate Symaira Cockpit product. Do not treat historical `symcockpit operate`/`scope` instructions as a current route: Cockpit is tune-only.

## Managed module installation

Build and install only the modules a selected Brain checkout needs:

```bash
symbrain setup --from-source /absolute/path/to/symaira-brain --modules operate,scope --json
```

This writes `symoperate` and `symscope` plus provenance sidecars to
`~/.symaira/bin/`. It does not modify Homebrew or historical release binaries,
does not grant macOS permissions, and does not expose any tool to an agent.

## MCP registration

Register the Brain-managed binaries directly when a non-Brain MCP host needs
them. This requires no Brain memory, gateway or profile:

```json
{
  "mcpServers": {
    "symoperate": {
      "command": "/Users/you/.symaira/bin/symoperate",
      "args": ["serve"]
    },
    "symscope": {
      "command": "/Users/you/.symaira/bin/symscope",
      "args": ["serve"]
    }
  }
}
```

Use real absolute paths. A profile-based Brain gateway integration additionally
requires explicit module enablement and an allowlisted tool surface; see the
Brain root README.

## Permissions and safety

- `symoperate` requires macOS Accessibility and Screen Recording permission for
  the process that actually launches it. Inspect first with `symoperate doctor`.
- `symscope` is local, read-only diagnostics. It starts no probe or watcher
  until an explicit tool invocation.
- Operate tools can inject input and read screen/window data. Start with the
  metadata-only allowlist (`version`, `permissions_status`, `get_policy`) and
  grant wider access only after a documented review.
- Never automate passwords, payments, destructive actions or permission dialogs
  without explicit human confirmation.

## Tune is separate

Symaira Cockpit remains responsible for hardware/system tuning. Its current and
historical release paths are not a substitute for the Brain-owned Operate/Scope
modules. This documentation does not publish a new Cockpit or Brain release;
signed artifact and package-manager migration remain separate gates.
