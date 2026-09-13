# Symaira macOS Agent Guide

> Combined setup for the `operate` (GUI actions) and `tune` (hardware tuning)
> families of `symcockpit`.

## Overview

Symaira provides two complementary MCP servers for full Mac control:

| Server | Purpose | Capabilities |
|--------|---------|-------------|
| `symoperate` | GUI automation | Screenshots, accessibility tree, mouse/keyboard, app/window control |
| `symaira-tune` | Hardware tuning | Thermals, brightness, power management, fan control |

## Individual Registration

### symoperate

```json
{
  "mcpServers": {
    "symoperate": {
      "command": "/path/to/symoperate",
      "args": ["serve"]
    }
  }
}
```

### symaira-tune

```json
{
  "mcpServers": {
    "symaira-tune": {
      "command": "/path/to/symaira-tune",
      "args": ["serve"]
    }
  }
}
```

## Combined Registration

For agents that need both GUI automation and hardware tuning:

```json
{
  "mcpServers": {
    "symoperate": {
      "command": "/path/to/symoperate",
      "args": ["serve"]
    },
    "symaira-tune": {
      "command": "/path/to/symaira-tune",
      "args": ["serve"]
    }
  }
}
```

## Recommended Agent Loop

1. **Discover** — use `symaira-scope` or `list_displays`/`list_windows` to understand the environment
2. **Query** — use `query_ui` or `find_ui` to locate targets
3. **Act** — use `click`, `type_text`, `press_keys` to interact
4. **Monitor** — use `wait_for` to observe state changes
5. **Tune** — use `symaira-tune` tools for thermal/power adjustments as needed

## Installation

### Homebrew (recommended)

```bash
brew install danieljustus/tap/symcockpit
```

One formula covers all three families — `operate`, `tune` and `scope` are
subcommands of the same binary.

### Build from source

```bash
# symoperate
git clone https://github.com/danieljustus/symaira-operate.git
cd symaira-operate && swift build -c release
cp .build/release/symoperate /usr/local/bin/

# symaira-tune
git clone https://github.com/danieljustus/symaira-tune.git
cd symaira-tune && swift build -c release
cp .build/release/symaira-tune /usr/local/bin/
```

## Permissions

Both servers require macOS permissions:

- **symoperate**: Accessibility + Screen Recording
- **symaira-tune**: Accessibility (for system-level queries)

Grant via:
```bash
symoperate permissions grant accessibility
symoperate permissions grant screen
```

## Safety

- Both servers are local-only, stdio-based, no remote transport
- `symoperate` blocks destructive UI actions by default (configurable via `set_policy`)
- Never automate passwords, payments, or permission dialogs without user confirmation
