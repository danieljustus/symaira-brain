# symaira-operate Architecture

## Overview

`symoperate` is a local macOS desktop-automation service for AI agents. It
exposes a stdio MCP server and a small CLI. The agent plans; symoperate executes
deterministic environment capabilities and returns a fresh observation — the same
model/environment split as Claude Computer Use, but exposing both screenshots and
structured `AXUIElement` data to reduce brittle coordinate-only behavior.

```
symoperate (executable)  →  SymOperateMCP  →  SymOperateCore
```

## Components

### `SymOperateCore`

- `PermissionService` — checks/prompts `Accessibility` and `Screen Recording`.
- `ScreenService` — captures the main display via ScreenCaptureKit, scales to an
  LLM-friendly size, returns PNG + transform metadata.
- `AccessibilityService` — traverses the frontmost app's `AXUIElement` tree,
  returns a bounded tree with ephemeral `element_id`s (bounded FIFO cache keyed
  by `snapshot_id`).
- `InputService` — synthesizes mouse/keyboard via `CGEvent`.
- `AppService` — enumerates apps/windows, launches apps, raises windows.
- `AutomationController` — orchestrates tools and enforces the safety guards
  (destructive-control refusal, secure-field block).

### `SymOperateMCP`

- Minimal stdio JSON-RPC/MCP transport: `initialize`, `tools/list`,
  `tools/call`, `ping`. Single place for tool schemas + argument validation.

### CLI (`symoperate`)

- `serve`, `doctor`, `version`, `history --json`, `permissions status|grant`.

## Tool contract

- `snapshot`/`list_displays` → PNG (base64) + coordinate transform metadata,
  by display ID or index.
- `query_ui`/`query_ui_ocr`/`find_ui` → fresh screenshot + frontmost UI tree
  with `element_id`s; OCR fallback and role/title/label/frame predicates for
  apps with weak Accessibility metadata.
- `click`/`drag` → accept raw coordinates **or** `snapshot_id + element_id`.
- `get_policy`/`set_policy` → read/augment the `ActionPolicy` allow/deny lists.
- `element_id`s are ephemeral; re-snapshot after any UI change.

## Distribution

Notarized direct download / Homebrew cask. The Mac App Store sandbox cannot grant
the system-wide Accessibility + Screen Recording automation this tool requires.
