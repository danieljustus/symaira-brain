# AGENTS.md — Symaira Guard (`symguard`)

This file documents coding conventions, project standards, and Symaira-specific rules for AI agents and humans contributing to `symaira-guard`.

**Design document:** [docs/intern/IDEA.md](docs/intern/IDEA.md)
**README:** [README.md](README.md)

---

## Project Structure

```
symaira-guard/
├── cmd/symguard/          # CLI entrypoint (main package): routing only
│   ├── main.go
│   ├── main_test.go
│   ├── decide/            # `symguard decide` command
│   │   └── command.go
│   ├── doctor/            # `symguard doctor` command
│   │   ├── checks.go
│   │   └── command.go
│   ├── grants/            # `symguard grants` command
│   │   └── command.go
│   ├── scan/              # `symguard scan` command
│   │   └── command.go
│   └── version/           # `symguard version` command
│       └── command.go
├── internal/              # Private packages (not importable outside this module)
│   ├── approval/          # Data contract for pending approval requests and human decisions
│   ├── archguard/         # Enforces allowed import directions between internal packages
│   ├── audit/             # Append-only audit log, evidence references, and security case bundles
│   ├── capability/        # Short-lived, purpose-bound capability tokens for headless callers
│   ├── config/            # TOML configuration schema and XDG path loader
│   ├── discovery/         # MCP server config discovery across AI client applications
│   ├── grant/             # Enumerable, scoped, revocable grant model
│   ├── model/             # Versioned, redaction-safe event contract
│   ├── output/            # Reporter interface and format-specific result renderers
│   ├── policy/            # Versioned rule catalog and evaluation engine
│   ├── proposal/          # Persisted request type for durable policy changes
│   ├── sequence/          # Stateful bounded-window detector for repetitive tool-call patterns
│   ├── spawn/             # Governs how stdio MCP servers are launched
│   └── update/            # Update checking via corekit
├── docs/                  # Documentation
│   └── intern/            # Internal design docs (IDEA.md)
├── go.mod
├── go.sum
├── Makefile
├── README.md
└── AGENTS.md              # This file
```

**Not yet created** (planned subsystems from the design doc — do not assume
these exist when navigating the repo): `internal/mcp/`, `internal/remote/`,
`internal/identity/`, `internal/pinning/`, `internal/diag/`,
`internal/redact/`, `internal/integrations/`. Create them only when the
corresponding feature is actually implemented.

**Rules:**
- Use `cmd/` for CLI entrypoints, `internal/` for all private logic.
- No `pkg/` directory — all library code stays internal until there is a proven external consumer.
- Each package under `internal/` maps to one subsystem. Keep package scope narrow.
- New subsystems get their own `internal/<name>/` directory with a single `<name>.go` file at minimum.

---

## Go Coding Standards

### Module and Dependencies

- Module path: `github.com/danieljustus/symaira-guard`
- Go version: 1.26+ (see `go.mod`)
- **Minimal dependencies.** Prefer the standard library. When a dependency is needed, justify it explicitly.
- Current external dependency: `github.com/BurntSushi/toml` (TOML parsing only).

### Error Handling

- Return errors, do not panic. Use `fmt.Errorf("context: %w", err)` for wrapping.
- Error messages start lowercase after the colon: `config: parse %s: %w`.
- Check errors immediately at the call site. Do not ignore errors.
- Use sentinel errors or typed errors only when callers need to branch on error type.

```go
// Good
cfg, err := LoadFrom(path)
if err != nil {
    return nil, fmt.Errorf("config: load: %w", err)
}

// Bad
cfg, _ := LoadFrom(path) // never ignore
```

### Naming

- Package names: short, lowercase, single-word (`config`, `policy`, `audit`).
- Exported types: PascalCase. Unexported: camelCase.
- Interface names: verb + `er` suffix when possible (`Reader`, `Matcher`, `Approver`).
- Constants: PascalCase for exported, camelCase for unexported.
- Avoid stuttering: `config.Config` is fine, `config.ConfigFile` is not.

### Package Layout

- Each package has a clear, single responsibility.
- Package-level `doc.go` or file-level doc comment explains purpose.
- Keep files under 400 lines. Split when a file grows beyond that.
- Group related functions. Put types before functions.

### Types and Interfaces

- Prefer small interfaces (1-3 methods).
- Accept interfaces, return structs.
- Use struct embedding for composition over inheritance.

```go
// Good — accept interface
func Evaluate(m Matcher, call ToolCall) Decision { ... }

// Good — return concrete type
func LoadFrom(path string) (*Config, error) { ... }
```

### Formatting

- Run `gofmt -w -s .` before committing. No exceptions.
- Use `go vet ./...` and `golangci-lint run ./...` (with `go vet` fallback) via `make lint`.

---

## Testing Conventions

### Table-Driven Tests

Use table-driven tests for all function-level unit tests:

```go
func TestValidate(t *testing.T) {
    tests := []struct {
        name    string
        input   *Config
        wantErr bool
    }{
        {"valid config", validConfig(), false},
        {"invalid decision", badDecisionConfig(), true},
    }
    for _, tt := range tests {
        t.Run(tt.name, func(t *testing.T) {
            err := validate(tt.input)
            if (err != nil) != tt.wantErr {
                t.Errorf("validate() error = %v, wantErr %v", err, tt.wantErr)
            }
        })
    }
}
```

### Test File Placement

- Co-locate tests with source: `config_test.go` next to `config.go`.
- Use `_test.go` suffix. No test files outside `_test.go`.

### Test Helpers

- Extract repeated setup into helper functions in the test file.
- Use `t.Helper()` in test helpers.
- Use `t.TempDir()` for filesystem tests — never hardcode temp paths.

### Coverage

- Aim for meaningful coverage, not 100% line coverage.
- Prioritize tests for: config parsing, policy matching, error paths, edge cases.
- CI runs `go test ./...`. Local verification is optional but recommended before pushes.

### Running Tests

```bash
make test          # or: go test ./...
go test -race ./... # race detector for concurrent code
```

---

## Git Workflow

### Commit Messages

Follow Conventional Commits. Format:

```
<type>: <description>

<body>

Refs #<issue>
```

**Types:** `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `ci`, `build`

**Examples:**

```
feat: add TOML config schema and XDG path loader

Implements config.Load() with XDG Base Directory support,
environment variable overrides, and TOML validation.

Closes #2
```

```
fix: handle missing config file gracefully

Return DefaultConfig() instead of error when config.toml does not exist.

Fixes #5
```

```
docs: add AGENTS.md with project conventions

Refs #3
```

**Rules:**
- Subject line: imperative mood, lowercase after type, no period, max 72 chars.
- Body: explain *what* and *why*, not *how*. Wrap at 80 chars.
- Reference issues with `Refs #N`, `Closes #N`, or `Fixes #N`.

### Branch Naming

- Feature branches: `feat/<short-description>` or `issue/<number>-<short-description>`
- Session branches: `session/YYYYMMDD-HHMM` (used for batch work)
- Hotfix branches: `fix/<short-description>`
- Examples: `issue/3-agents-md`, `feat/scanner-mvp`, `session/20260626-180801`

### Pushing

- Push to origin regularly. Use `git push` for existing upstreams.
- New branches: `git push -u origin <branch>`.
- Do not force-push shared branches.

---

## Symaira-Specific Rules

### Room ↔ Guard Boundary (Do Not Weaken)

| | Guard | Room |
|---|---|---|
| Question | May this concrete *call* happen right now? | Is this *operation* approved, and where is the record that it happened? |
| Unit | single tool call | run, checkpoint, decision |
| Position | in the data path (proxy) | beside the data path (bookkeeping) |
| Risk classification | core competence | none |
| Record | hash-chain call audit | signed room journal |

Guard decides **whether** a call must be asked about or blocked (risk classification,
allow/ask/deny, schema pinning, call-level audit). The room is **where** an operation is
asked about and recorded (participants, signed work journal, approvals with scope and
TTL). Guard may use the room as its approval backend through the frozen contract; the
room performs no risk classification and never sits in the data path.

> Feature requests for room-side risk classes, per-call ask/deny or policy evaluation
> belong in this repo (`symaira-guard`) and must be implemented here, not in the room.
> See [`symaira-room`'s approval backend contract](https://github.com/danieljustus/symaira-room/blob/main/docs/approval-contract.md)
> for the frozen interface `symguard` uses when the room is the approval backend.

### Binary Name

The binary is always named `symguard`. Do not use alternative names.

```bash
go build -o symguard ./cmd/symguard
```

### Standalone-First

`symguard` must work without any other Symaira tool installed:

- No compile-time imports from sibling repos (`symvault`, `symmemory`, `symscope`, etc.).
- Optional runtime integration via `exec.LookPath`, MCP discovery, HTTP APIs, or config paths.
- If an optional tool is missing, provide a clear fallback or skip gracefully — never crash.

```go
// Good — graceful fallback
func initSymvault() *SymvaultClient {
    path, err := exec.LookPath("symvault")
    if err != nil {
        log.Println("symvault not found, secret mediation disabled")
        return nil
    }
    return newClient(path)
}
```

### XDG Paths

Follow XDG Base Directory Specification:

| Purpose | Path | Env Override |
|---------|------|--------------|
| Config | `~/.config/symguard/config.toml` | `SYMGUARD_CONFIG` or `$XDG_CONFIG_HOME/symguard/config.toml` |
| Data | `~/.local/share/symguard/` | `$XDG_DATA_HOME/symguard/` |
| Cache | `~/.cache/symguard/` | `$XDG_CACHE_HOME/symguard/` |

### Zero Stdio Pollution (MCP Mode)

When running as an MCP proxy (`symguard proxy`):

- stdin/stdout are the MCP JSON-RPC transport.
- All diagnostic output goes to stderr.
- Never write non-JSON-RPC data to stdout in proxy mode.
- Use `log.SetOutput(os.Stderr)` or explicit stderr writers.

```go
// Good — stderr for diagnostics
fmt.Fprintf(os.Stderr, "config: warning: unknown key %q\n", key)

// Bad — pollutes MCP transport
fmt.Printf("config: warning: unknown key %q\n", key)
```

### Clear Fallback for Missing Tools

When integrating with optional Symaira tools:

- Check availability at startup or on first use.
- Print a clear message explaining what is unavailable and what functionality is degraded.
- Continue operating with reduced capability rather than failing.

### Code Belongs in the Public Core

- No Pro, tenant, billing, or hosted-service code in this repository.
- Remote access feature code belongs here (it is part of `symguard`), but hosted relay infrastructure belongs elsewhere.

---

## Documentation

- Keep README.md updated with build instructions, usage, and project overview.
- Design decisions and architecture go in `docs/intern/IDEA.md`.
- Code comments explain *why*, not *what*. The code itself shows *what*.
- Package-level doc comments are mandatory for every package.

---

## Quick Reference

```bash
# Build
make build

# Test
make test

# Lint
make lint

# Format
make fmt

# Full local check
go vet ./... && go test ./... && go build -o symguard ./cmd/symguard
```

---

*This file is a living document. Update it when conventions change.*
