# Symaira Scope Discovery

> How `symoperate` surfaces in `symaira-scope`'s MCP inventory.

## Registration

`symoperate` is discoverable by `symaira-scope` when registered as an MCP server.

### Client Registration (Claude Desktop, OpenCode, etc.)

Add to your MCP client config:

```json
{
  "mcpServers": {
    "symoperate": {
      "command": "symoperate",
      "args": ["serve"]
    }
  }
}
```

### Discovery via `symaira-scope`

If `symaira-scope` is installed, it can enumerate registered MCP servers:

```bash
symscope mcp list
```

This should show `symoperate` among registered servers.

### Manual Verification

```bash
# Verify symoperate is installed and working
symoperate doctor

# Check permissions
symoperate permissions status
```

## Server Capabilities

When registered, `symoperate` exposes these capabilities:

- **Tools**: 20 tools (snapshot, query_ui, find_ui, click, type_text, etc.)
- **Protocol**: MCP 2024-11-05
- **Transport**: stdio (JSON-RPC)

## Integration Notes

- `symoperate` is the GUI automation sibling of `symaira-tune` (hardware tuning)
- Both can run simultaneously for full Mac control
- See `docs/macos-agent-guide.md` for combined setup
