# AGENTS.md — Symaira Guard (`guard/` module of `symaira-brain`)

This file documents Guard-specific rules for AI agents and humans contributing to the guard module of `symaira-brain` (formerly the standalone `symaira-guard` repository; absorbed 2026-08-21). General coding, testing and Git conventions are those of the root [AGENTS.md](../AGENTS.md).

The Go implementation was removed after the Rust cutover (2026-11-04). Guard now lives in two Rust crates of the root workspace (ADR 0011):

| Crate | Role |
|---|---|
| `rust/symbrain-guard-core` | Pure deterministic model and static policy kernel |
| `rust/symguard-cli` | Shared Guard command handlers (`symbrain guard …`) and the standalone `symguard` binary |

This directory keeps the Guard documentation, assets and the recorded Go-oracle inputs that the Rust tests replay (`guard/scripts/*-oracle`). Recorders that need a Go toolchain only run from a checkout that predates the removal.

**README:** [README.md](README.md)

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
> belong in the guard module of `symaira-brain` and must be implemented here, not in the room.

### Binary Name

The binary is always named `symguard`. Do not use alternative names.

```bash
cargo build --release --locked -p symguard-cli   # target/release/symguard
```

### Standalone-First

`symguard` must work without any other Symaira tool installed:

- No compile-time imports from sibling repos (`symvault`, `symmemory`, `symscope`, etc.).
- Optional runtime integration via PATH lookup, MCP discovery, HTTP APIs, or config paths.
- If an optional tool is missing, provide a clear fallback or skip gracefully — never crash.

### XDG Paths

Follow XDG Base Directory Specification:

| Purpose | Path | Env Override |
|---------|------|--------------|
| Config | `~/.config/symguard/config.toml` | `SYMGUARD_CONFIG` or `$XDG_CONFIG_HOME/symguard/config.toml` |
| Data | `~/.local/share/symguard/` | `$XDG_DATA_HOME/symguard/` |
| Cache | `~/.cache/symguard/` | `$XDG_CACHE_HOME/symguard/` |

### Zero Stdio Pollution (MCP Mode)

When running as an MCP proxy:

- stdin/stdout are the MCP JSON-RPC transport.
- All diagnostic output goes to stderr.
- Never write non-JSON-RPC data to stdout in proxy mode.

### Clear Fallback for Missing Tools

When integrating with optional Symaira tools:

- Check availability at startup or on first use.
- Print a clear message explaining what is unavailable and what functionality is degraded.
- Continue operating with reduced capability rather than failing.

### Code Belongs in the Public Core

- No Pro, tenant, billing, or hosted-service code in this repository.
- Remote access feature code belongs here (it is part of `symguard`), but hosted relay infrastructure belongs elsewhere.

---

## Quick Reference

```bash
cargo test -p symbrain-guard-core -p symguard-cli --locked
make guard-oracle-check guard-scan-oracle-test   # sealed Guard fixtures
make rust-check                                 # complete root gate
```

---

*This file is a living document. Update it when conventions change.*
