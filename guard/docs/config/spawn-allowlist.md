# Spawn Allowlist & Plaintext-Secret Checks

`symguard` governs not only *what* MCP servers are asked to do (the policy
layer) but also *how* they are launched. A stdio MCP server launch consists
of an executable path, an argv, and an environment — and the environment is
where API keys travel. This page documents the two surfaces that cover the
launch boundary: the **spawn allowlist** and the **`symguard doctor`
plaintext-secret check**.

## Spawn allowlist

The spawn allowlist gates which stdio MCP servers may be started at all.
It is **deny by default**: a server whose launch does not match an entry is
not spawnable, and with no `[spawn]` section nothing is spawnable.

Entries live in `~/.config/symguard/config.toml`:

```toml
[spawn]

# Allow /usr/local/bin/node but only when launched as a filesystem server.
[[spawn.allowlist]]
path = "/usr/local/bin/node"
argv_prefix = ["server.js", "--port"]

# Allow uvx with any arguments (no argv_prefix).
[[spawn.allowlist]]
path = "/opt/homebrew/bin/uvx"
```

Matching rules:

- `path` is the **absolute** path of the executable. Relative paths are
  rejected at config load (`config: validate ...`), and a discovered server
  whose command is not absolute can never match an entry.
- `argv_prefix` is optional. When present, it must be a prefix of the
  server's arguments; when absent, any arguments match. Paths are compared
  after `filepath.Clean`, so redundant `.`/`..` elements are harmless.
- Only **stdio** servers are gated. HTTP servers are reached by URL and have
  no executable to allowlist.
- To find the absolute path of a command: `command -v npx` (or
  `which npx`).

Gating happens in `internal/spawn` (`spawn.Allowlist`); the config schema
lives in `internal/config` (`[spawn] allowlist`). `symguard doctor` reports
the verdict for every discovered server (see below); the proxy layer will
enforce it before starting a server.

## `symguard doctor` plaintext-secret check

`doctor` scans MCP server configs discovered from your AI clients (Hermes,
Claude Desktop, Cursor, VS Code, OpenCode) and flags environment values that
look like plaintext secrets:

```
Plaintext secret risk:
  api-tool (claude-desktop): env API_KEY, TOKEN stored as plaintext values in the client config
  symguard reports this risk but is not a secret store — move these values to symvault and reference them at launch time.
```

Heuristics (`internal/discovery.LooksLikeSecret`):

- A value is only flagged when it is non-empty **and** not a variable
  reference (`$NAME` or `${NAME}`) — referenced values are not plaintext.
- Flagged when the **key name** contains `API_KEY`, `TOKEN`, `SECRET`,
  `PASSWORD`, `PASSWD`, `CREDENTIAL`, `PRIVATE_KEY`, or `AUTH`
  (case-insensitive).
- Flagged when the **value** starts with a common literal secret prefix
  (`sk-`, `sk_`, `ghp_`, `gho_`, `AKIA`, `xoxb-`, `xoxp-`).

## Boundary: report and gate, do not store

`symguard` reports and gates the launch surface; it is **not** a secret
store. Secrets belong in `symvault`; the intended pattern is to reference
them (`${VAR}`) or inject them at launch time, so they never sit in
plaintext in a client's MCP config. If `doctor` flags a config, move the
value to `symvault` and reference it instead.
