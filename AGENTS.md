# AGENTS.md — Symaira Brain (`symbrain`)

This file documents coding conventions, project standards, and Symaira-specific rules for AI agents and humans contributing to `symaira-brain`.

**README:** [README.md](README.md)

---

## Product Boundary

**Symbrain is the portable agent-context layer.** It multiplexes the three
Symaira *state cores* — `symvault` (credentials), `symmemory` (memory/entities),
`symskills` (skill SSOT) — behind one MCP gateway, with one profile per
harness connection controlling what that harness is allowed to see.

Symbrain is explicitly **not**:

- **A generic MCP hub or aggregator.** It only multiplexes the state cores.
  Tools like `symfetch`, `symseek`, `symprint`, `symfritz`, etc. are bound
  directly to the harness by the user — symbrain does not proxy them.
- **A second `symguard`.** See "Brain ↔ Guard Boundary" below.
- **A second `symskills`.** Skill rendering/installation stays entirely in
  symskills; symbrain only orchestrates it (shells out, parses `--json`).
- **A memory store.** Symbrain persists no memories and no secrets itself. It
  only holds profiles, the instructions source, and an audit log.
- **A GUI** in v0.1.0 (though native SwiftUI apps now exist in `Sources/` as
  the macOS and iOS clients for the CLI).

### Brain ↔ Guard Boundary (verbatim — do not weaken)

Both tools sit between a harness and its tool servers, but at a different
question:

| | **Brain** | **Guard** |
|---|---|---|
| Question | *What is this agent even allowed to see?* | *Is this specific call allowed to happen right now?* |
| Mechanism | **Capability shaping**: filtered `tools/list`, servers on/off per profile, modes like vault `request_only` | **Conduct policing**: risk classification, allow/ask/deny/redact per call, human approval, schema pinning, hash-chain audit |
| Scope | Only the Symaira state cores | Any MCP server, any client |
| Timing | At handshake / catalog build | On every tool call |
| Audit | Lightweight JSONL log (who/what/when, redacted) | Tamper-evident audit (hash chain) |

Consequences:

1. Symbrain implements **no** approval prompts, **no** risk classes, **no**
   schema pinning. A user who needs those puts `symguard` in front.
2. Feature requests for per-call ask/deny policies, risk classification, or
   approval flows belong in `symaira-guard`, not here — redirect them.
3. What symbrain does today must stay guard-compatible: stable deterministic
   tool names (guard pins schemas), snake_case JSON, zero stdio pollution.

---

## Project Structure

```
symaira-brain/
├── cmd/symbrain/          # CLI entrypoint (main package)
│   └── main.go
├── internal/              # Private packages (not importable outside this module)
│   ├── profile/           # Profile schema (TOML), loading, validation, defaults
│   ├── policy/            # Exposure policy: allow/deny lists, mode presets
│   ├── broker/            # MCP client: spawn, initialize, tools/list, tools/call,
│   │                      #   child lifecycle (health, crash-restart with backoff)
│   ├── gateway/           # MCP server side: handshake, catalog merge,
│   │                      #   namespacing, routing, error mapping
│   ├── catalog/           # Tool catalog: namespacing rules, collision detection
│   ├── audit/             # JSONL audit log, redaction rules
│   ├── instructions/      # Canonical instructions source + managed blocks
│   ├── adapter/           # One small module per harness (claude, codex, cursor,
│   │                      #   opencode, gemini): instructions + MCP config
│   ├── harness/           # Harness registry: config paths, formats, backup
│   └── skills/            # symskills orchestration (CLI shell-out, --json)
├── Sources/               # Native SwiftUI apps (macOS + iOS)
│   ├── SymBrainCore/      # Static library: CLI client, models, view models
│   ├── SymBrainApp/       # macOS application (NavigationSplitView dashboard)
│   └── SymBrainMobile/    # iOS companion (read-only overview)
├── Tests/                 # Swift unit tests
│   └── SymBrainCoreTests/ # Decoding tests for CLI JSON shapes
├── docs/                  # Local planning docs (git-ignored, not part of the module)
├── project.yml            # Xcode project generation (xcodegen)
├── go.mod, go.sum
├── Makefile
├── README.md
├── LICENSE                # Apache-2.0
└── AGENTS.md              # This file
```

**Rules:**
- Use `cmd/` for the CLI entrypoint, `internal/` for all private logic.
- No `pkg/` directory — all library code stays internal until there is a
  proven external consumer.
- Each package under `internal/` maps to one subsystem. Keep package scope
  narrow.
- New subsystems get their own `internal/<name>/` directory with at least a
  `doc.go` package comment.

---

## Go Coding Standards

### Module and Dependencies

- Module path: `github.com/danieljustus/symaira-brain`
- Go version: 1.26+ (see `go.mod`), `CGO_ENABLED=0`.
- Depends on `symaira-corekit` (pinned to a tagged release, no `replace`
  directive on `main`) for: `configkit`, `logkit`, `exitcodes`, `fsutil`,
  `versionkit`, `updatecheck`.
- **Minimal dependencies.** Prefer the standard library. When a dependency is
  needed, justify it explicitly.

### Error Handling

- Return errors, do not panic. Use `fmt.Errorf("context: %w", err)` for
  wrapping.
- Check errors immediately at the call site. Do not ignore errors.
- Use `corekit/exitcodes` to map errors to the process exit code.

### Naming

- Package names: short, lowercase, single-word (`profile`, `policy`, `audit`).
- Exported types: PascalCase. Unexported: camelCase.
- Avoid stuttering: `profile.Profile` is fine, `profile.ProfileConfig` is not.

### Package Layout

- Each package has a clear, single responsibility.
- Package-level `doc.go` or file-level doc comment explains purpose.
- Keep files under 400 lines. Split when a file grows beyond that.

### Formatting

- Run `gofmt -w -s .` before committing. No exceptions.
- Use `go vet ./...` and `golangci-lint run ./...` (with `go vet` fallback)
  via `make lint`.

---

## Testing Conventions

- Table-driven tests for function-level unit tests.
- Co-locate tests with source: `config_test.go` next to `config.go`.
- Use `t.TempDir()` for filesystem tests — never hardcode temp paths.
- Prioritize tests for: profile/policy evaluation (table-driven, every preset
  matrix), catalog collision detection, adapter output (golden files per
  harness), instructions managed-block idempotency (sync twice = identical
  file).
- Integration tests against the broker use a fake MCP child binary
  (JSON-RPC over stdio) checked into the repo — no test may assume a real
  Symaira binary is present (CI containers have none).
- CI runs `go test -race ./...`. Local verification is optional but
  recommended before pushing.

```bash
make test          # or: go test ./...
make test-race      # go test -race ./...
```

---

## Git Workflow

### Commit Messages

Follow Conventional Commits. Format:

```
<type>: <description>

<body>

Closes #<issue>
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, `build`

Subject line: imperative mood, lowercase after type, no period, max 72 chars.
Body explains *what* and *why*, not *how*. Reference issues with `Refs #N`,
`Closes #N`, or `Fixes #N`.

### Branch Naming

- Feature branches: `feat/<short-description>` or `issue/<number>-<short-description>`
- Session branches: `session/YYYYMMDD-HHMMSS` (used for batch work)
- Hotfix branches: `fix/<short-description>`

---

## Symaira-Specific Rules

### Binary Name

The binary is always named `symbrain`. Do not use alternative names.

```bash
go build -o symbrain ./cmd/symbrain
```

### Standalone-First

`symbrain` must work without any other Symaira tool installed:

- **No compile-time imports** of sibling repos (`symvault`, `symmemory`,
  `symskills`, `symguard`, etc.). The MCP client (broker) that talks to them
  is implemented locally in `internal/broker`.
- Children are found at runtime via `exec.LookPath` with a timeout, never
  assumed to be present.
- If a child binary is missing, the corresponding server is simply left out
  of the catalog and `symbrain doctor` explains why — never a hard error.

```go
// Good — graceful degradation
path, err := exec.LookPath("symvault")
if err != nil {
    // server omitted from catalog; doctor reports it as a warning, not fatal
    return nil
}
```

### XDG Paths

| Purpose | Path | Env prefix |
|---------|------|--------------|
| Config | `~/.config/symbrain/config.toml` | `SYMBRAIN_*` |
| Data (audit log) | `~/.local/share/symbrain/` | `SYMBRAIN_*` |
| Cache | `~/.cache/symbrain/` | `SYMBRAIN_*` |

### Zero Stdio Pollution (`serve` mode)

When running as an MCP gateway (`symbrain serve --profile <name>`):

- stdin/stdout are the MCP JSON-RPC transport.
- All diagnostic output goes to stderr (`corekit/logkit`, which logs to
  stderr).
- Never write non-JSON-RPC data to stdout while serving.

### snake_case JSON

All MCP/API JSON payloads and every `--json` CLI output use snake_case field
names. Go structs use idiomatic CamelCase with `json:"snake_case"` tags.

### Exit Codes

- `0`: success
- `1`: general runtime error
- `2`: usage or configuration error

### Code Belongs in the Public Core

- No Pro, tenant, billing, or hosted-sync code in this repository. Multi-device
  sync is a future `symaira-brain-pro` concern, not this repo's.

---

## Documentation

- Keep README.md updated with build instructions, usage, and project overview.
- Code comments explain *why*, not *what*.
- Package-level doc comments are mandatory for every package.

---

## Quick Reference

```bash
# Build
make build

# Test
make test
make test-race

# Lint
make lint

# Format
make fmt

# Full local check
go vet ./... && go test -race ./... && go build -o symbrain ./cmd/symbrain
```

---

*This file is a living document. Update it when conventions change.*
