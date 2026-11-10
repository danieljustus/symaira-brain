# AGENTS.md — Symaira Brain (`symbrain`)

This file documents coding conventions, project standards, and Symaira-specific rules for AI agents and humans contributing to `symaira-brain`.

**README:** [README.md](README.md)

## Supported Platform Scope (2026-10-29, #990)

Product targets are **macOS on Apple Silicon, iOS, and Web**. Native Windows and macOS Intel are unsupported and are no longer required for product parity, tests, or release artifacts. Linux remains supported for CI/server use and CLI binaries where applicable; it is not an Apple/Web product target. Keep these boundaries explicit in new port contracts and CI plans. Historical ADRs, fixtures, and receipts retain their original meaning; add dated superseding notes rather than rewriting their evidence.

---

## Product Boundary

**Accepted target: [PB-2026-09-09](docs/adr/0002-product-boundaries.md).** Brain owns portable agent context/control and the optional Browse, Operate and Scope modules. Browse remains actively developed; Operate is retained, not automatically replaced by Cua or Hermes. Brain GUI/CLI become the integrated credential-management surface, backed by the independently usable `symvault` service. Browser and OS-automation workers remain optional and permission-isolated; no master keys enter the gateway. Module source/direct-consumer cutovers are completed; release-distribution migration and the credential-management UI replacement remain pending. This target supersedes conflicting state-core-only and no-GUI scope restrictions below; existing commands and safety contracts remain until tested cutovers.

**Symbrain is the portable agent-context layer.** It exposes the three Symaira
*state cores* — credentials, memory/entities, and the skill SSOT — behind one
MCP gateway, with one profile per harness connection controlling what that
harness is allowed to see.

Two of the three now live in this repository: memory (`rust/symbrain-memory`) and
skills (`rust/symbrain-skills`) were absorbed by the repo consolidation on
2026-08-21 (steps 4 and 7), together with guard (`guard/`). Only `symvault`
remains a separate process, deliberately — the secret store assumes its caller
is untrusted. The separate credential process is defense-in-depth alongside service-side authorization, scoped grants and protected IPC; it is not a complete same-user sandbox.

Symbrain is explicitly **not**:

- **A generic MCP hub or aggregator.** Symbrain is not a convenience bundle —
  aggregation without a policy gain is out (ADR 0001). It bundles the state
  cores and, where a profile explicitly names a foreign server, passes it
  through for exposure control and audit. Other tools (`symdesk`, `symbrowse`,
  `symcockpit`, `symfritz`, …) are bound directly to the harness by the user
  unless a profile declares them.
- **A second `symguard`.** See "Brain ↔ Guard Boundary" below.
- **A second `symskills`.** Skill rendering/installation was absorbed on
  2026-08-21 and now runs in-process through `rust/symbrain-skills` (driven by
  `symbrain sync` / `symbrain skills`). No external `symskills` binary is resolved or
  executed, and the CLI surface lives here as `symbrain skills`.
- **A remote memory or credential store.** Memory and skills are embedded
  in-process (absorbed 2026-08-21); credentials stay behind the separate
  `symvault` process on purpose. symbrain brokers the external vault and
  owns the embedded memory + skills stores.
- **A second general credential service.** Brain owns human administration, including the planned credential UI, but delegates credential storage, cryptography and authorization to the separate service. Human administration must not automatically expand agent MCP exposure.

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
   approval flows belong in the `guard/` module, not in the brain packages —
   redirect them. (Guard was a separate repository until 2026-08-21; the
   boundary is now a module boundary, and is no weaker for it.)
3. What symbrain does today must stay guard-compatible: stable deterministic
   tool names (guard pins schemas), snake_case JSON, zero stdio pollution.

---

## Project Structure

The Go implementation was removed after the Rust cutover (2026-11-04). Its
observable behavior stays pinned by sealed, replayed fixtures
(`scripts/oracle-freeze`, `migration/evidence/oracle-freeze-781`).

```
symaira-brain/
├── rust/                  # Root Cargo workspace members
│   ├── symbrain-cli/      # `symbrain` binary: argument parsing and command dispatch
│   ├── symbrain-core/     # Shared primitives: config, XDG paths, exit codes, output
│   ├── symbrain-policy/   # Profile schema (TOML), exposure policy, mode presets
│   ├── symbrain-broker/   # MCP client: child spawn, lifecycle, crash-restart
│   ├── symbrain-gateway/  # MCP server side: handshake, catalog merge, routing
│   ├── symbrain-mcp/      # JSON-RPC and MCP stdio framing
│   ├── symbrain-catalog/  # Tool catalog: namespacing rules, collision detection
│   ├── symbrain-audit/    # JSONL audit log, redaction rules
│   ├── symbrain-instructions/ # Canonical instructions source + managed blocks
│   ├── symbrain-adapter/  # Per-harness instruction adapters
│   ├── symbrain-harness/  # Harness registry: config paths, formats, backup
│   ├── symbrain-memory/   # Absorbed symmemory: store, entities, consolidation
│   ├── symbrain-skills/   # Absorbed symskills: skill SSOT, render and install
│   ├── symbrain-activity/ # Bounded activity read contracts
│   ├── symbrain-patterns/ # Recurring gateway episode promotion
│   ├── symbrain-usage/    # AI provider usage reporting
│   ├── symbrain-managed/  # Managed core manifest, verification, extraction
│   ├── symbrain-guard-core/ # Guard model and static policy kernel
│   ├── symguard-cli/      # Guard command handlers and standalone `symguard`
│   ├── symaira-cutover/   # One-time backup of Go-era state on the first Rust run
│   └── test-support/      # Shared test helper module (coverage environment)
├── Cargo.toml, Cargo.lock # Root Rust workspace and locked dependencies
├── rust-toolchain.toml    # Pinned Rust toolchain and components
├── browse/                # Optional web worker: its own Cargo workspace (symbrowse)
├── operate/               # Optional OS-automation worker sources (SwiftPM)
├── scope/                 # Optional machine-inventory/diagnostics sources (SwiftPM)
├── Sources/               # Native SwiftUI apps (macOS + iOS)
│   ├── SymBrainCore/      # Static library: CLI client, models, view models
│   ├── SymBrainApp/       # macOS application (NavigationSplitView dashboard)
│   └── SymBrainMobile/    # iOS companion (read-only overview)
├── Tests/                 # Swift unit tests
│   └── SymBrainCoreTests/ # Decoding tests for CLI JSON shapes
├── history/               # Swift package symcockpit-history (SymCockpitHistory library)
├── guard/                 # Guard module docs, assets and recorded oracle inputs
│                          #   (absorbed 2026-08-21, relicensed MIT → Apache-2.0)
├── fuzz/                  # cargo-fuzz targets for MCP framing
├── migration/             # Contract matrix and immutable migration evidence
├── evidence/              # Retained release and rollback artifact evidence
├── assets/                # App icon, branding, DMG resources, screenshots
├── scripts/               # Release, coverage, fixture-gate and replay tooling
├── docs/                  # Tracked: docs/adr/ and docs/handoffs/end-work-cloud.md;
│                          #   all other docs/ content is ignored private planning
├── project.yml            # Xcode project generation (xcodegen)
├── Makefile
├── README.md, CHANGELOG.md, MIGRATION-RUST.md, MEMORY-SYNC-MIGRATION.md
├── LICENSE                # Apache-2.0
├── CLAUDE.md, GEMINI.md   # Managed agent-instruction files that point here
└── AGENTS.md              # This file
```

**Crate layout rules:**
- `symbrain-cli` owns argument parsing and dispatch only; behavior lives in
  library crates under `rust/`.
- Each crate maps to one subsystem. Keep crate scope narrow and expose the
  smallest public API its callers need.
- New subsystems get their own `rust/<name>/` crate with a crate-level `//!`
  doc comment, added to the root workspace.

---

## Rust Coding Standards

### Toolchain and Dependencies

- Use the toolchain pinned in `rust-toolchain.toml`. Build and test with
  `--locked`; never update `Cargo.lock` as a side effect.
- **Minimal dependencies.** Prefer the standard library. When a dependency is
  needed, justify it explicitly; `deny.toml` (cargo-deny) and cargo-audit gate
  licenses, sources and advisories.
- No `unsafe` unless unavoidable and documented; crates default to
  `#![deny(unsafe_code)]`.

### Error Handling

- Return `Result`, do not panic in production paths. Add context where an
  error crosses a boundary.
- Check errors at the call site. Do not ignore them silently.
- Map errors to the process exit code through `symbrain_core::exit`.

### Naming

- Crate and module names: short, lowercase, snake_case modules.
- Types: `UpperCamelCase`; functions and variables: `snake_case`.
- Avoid stuttering: `profile::Profile` is fine, `profile::ProfileConfig` is not.

### File Layout

- Each module has a clear, single responsibility and a `//!` doc comment.
- Keep files under 400 lines. Split when a file grows beyond that.

### Formatting and Lint

- Run `make fmt` (`cargo fmt --all`) before committing.
- `make lint` runs the rustfmt check and `cargo clippy --all-targets
  --all-features -- -D warnings`.

---

## Testing Conventions

- Table-driven tests for function-level unit tests.
- Co-locate unit tests with the module (`#[cfg(test)]` or a sibling
  `*_tests.rs`); process-level tests live in each crate's `tests/`.
- Use `tempfile::TempDir` for filesystem tests and disposable HOME/XDG roots
  for process tests — never hardcode temp paths or touch real user state.
- Prioritize tests for: profile/policy evaluation (table-driven, every preset
  matrix), catalog collision detection, adapter output (golden files per
  harness), instructions managed-block idempotency (sync twice = identical
  file).
- Integration tests against the broker use a fake MCP child binary
  (JSON-RPC over stdio) checked into the repo — no test may assume a real
  Symaira binary is present (CI containers have none).
- Go-recorded oracle fixtures are immutable evidence. A deliberate change to a
  frozen fixture needs a reviewed re-seal (`make oracle-freeze-seal`); the
  gate fails on any unregistered or changed fixture.
- CI runs `make rust-check parity-smoke init-differential` on Linux and macOS,
  plus the browse, Swift, GUI, fuzz and coverage jobs. Local verification is
  optional but recommended before pushing.

```bash
make test          # cargo test --workspace --all-features --locked
make rust-check    # complete gate
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
cargo build --release --locked -p symbrain-cli   # target/release/symbrain
```

### Standalone-First

`symbrain` must work without any other Symaira tool installed:

- **No compile-time imports of separately released tools.** Since the
  consolidation this means `symvault` and any tool outside this repo; memory,
  skills and guard are now packages here and are linked directly. The MCP
  client (broker) that talks to external children lives in `rust/symbrain-broker`.
- Children are found at runtime via a PATH lookup with a timeout, never
  assumed to be present.
- If a child binary is missing, the corresponding server is simply left out
  of the catalog and `symbrain doctor` explains why — never a hard error.

> **Verified (2026-08-25).** Skill rendering and installation for `symbrain sync`
> run in-process through the embedded skills runner — no
> external `symskills` binary is resolved or executed. Removing the archived
> binary from `PATH` does not skip any requested target; render/install
> failures surface in JSON and human output with a non-zero exit status.

```rust
// Good — graceful degradation
let Some(path) = path_lookup(OsStr::new("symvault")) else {
    // server omitted from catalog; doctor reports it as a warning, not fatal
    return None;
};
```

### Secret References

Config values that name a secret (e.g. `jwt.secret`) accept a `symvault://<path>`
URI, resolved via a `symvault get <path> --print` subprocess (5s timeout,
env-var fallback). `vault://<path>` is accepted as a deprecated alias for
backward compatibility — both resolve to the identical path and behavior.
New configs and error messages should use `symvault://`; see the secret
reference handling in `rust/symbrain-cli/src/memory_cli/serve.rs` and
`rust/symbrain-usage/src/provider_config/references.rs`.

### XDG Paths

| Purpose | Path | Env prefix |
|---------|------|------------|
| Config | `~/.config/symbrain/config.toml` | `SYMBRAIN_*` |
| Data (audit log) | `~/.local/share/symbrain/` | `SYMBRAIN_*` |
| Cache | `~/.cache/symbrain/` | `SYMBRAIN_*` |

### Zero Stdio Pollution (`mcp` mode)

When running as an MCP gateway (`symbrain mcp --profile <name>`; `serve` remains
a deprecated compatibility alias):

- stdin/stdout are the MCP JSON-RPC transport.
- All diagnostic output goes to stderr.
- Never write non-JSON-RPC data to stdout while serving.

### snake_case JSON

All MCP/API JSON payloads and every `--json` CLI output use snake_case field
names. Rust structs use `snake_case` fields with serde; keep JSON field names
stable (guard pins schemas and the frozen fixtures pin the bytes).

### Exit Codes

- `0`: success
- `1`: general runtime error
- `2`: usage or configuration error

### Code Belongs in the Public Core

- No tenant, billing, or hosted-sync code in this repository. There is no Pro
  edition and no hosted or multi-tenant sync service. Peer-to-peer
  `symbrain memory sync` between the user's own stores (including a
  self-hosted `symbrain memory serve` peer and the encrypted relay) is shipped
  and in scope (maintainer decision 2026-11-04, #844); a hosted or tenant sync
  offering stays out of scope, not deferred elsewhere.

### Native UI

`Sources/SymBrainApp` and `Sources/SymBrainMobile` follow the Symaira
Interface Guidelines shipped with the pinned `symaira-corekit` (AppKit modules, `appkit/`): read its
`GUIDELINES.md`, then `DESIGN.md` and `docs/workspace-foundation.md`, and
review a UI change with `python3 <symaira-corekit>/appkit/scripts/gui-review.py
Sources/SymBrainApp` plus the manual checklist in GUIDELINES.md section 13.
iOS keeps native `NavigationSplitView` and `TabView`; the drag-resizable
workspace is macOS only (GUIDELINES.md section 12).

---

## Documentation

- Keep README.md updated with build instructions, usage, and project overview.
- Code comments explain *why*, not *what*.
- Crate-level doc comments are mandatory for every crate.

---

## CLI Command Reference

| Command | Purpose |
|---|---|
| `symbrain init` | Create XDG directories, default config, and example profiles |
| `symbrain doctor [--json]` | Check environment, config, profiles, and child binaries |
| `symbrain setup` | Download and install pinned core binaries to `~/.symaira/bin` |
| `symbrain setup --from-source DIR [--modules browse,operate,scope]` | Build optional module binaries (browse/operate/scope) from in-repo sources at `DIR` and install them into the managed directory with a provenance sidecar; `doctor`/`setup --fix` treat this as intentional and skip unless `--force-release` |
| `symbrain profile list \| show \| add \| remove` | Manage profiles under `~/.config/symbrain/profiles/` |
| `symbrain config` | Inspect and edit the global config (path, get, set) |
| `symbrain harness list [--project DIR]` | Inspect every known harness and its registered MCP servers |
| `symbrain harness health [--harness NAME] [--project DIR]` | Probe the MCP `initialize` handshake of registered servers |
| `symbrain usage` | AI subscription/token usage per provider |
| `symbrain mcp --profile <name> \| --profile-file <path>` | Run the MCP gateway over stdio (`serve` is a deprecated alias) |
| `symbrain install --harness <name> --profile <name> [--project DIR] [--dry-run]` | Register symbrain with a harness |
| `symbrain uninstall --harness <name> [--project DIR] [--dry-run]` | Remove symbrain from a harness |
| `symbrain sync [--project DIR] [--dry-run] [<harness>...]` | Push instructions/skills to installed harnesses |
| `symbrain memory` | Operate the embedded memory store (list, search, set, delete, rules, query-log) |
| `symbrain memory sync --remote <url> [--pull\|--push]` | Peer-to-peer sync between the user's own stores (self-hosted peer, optional encrypted relay) |
| `symbrain memory serve [--port <n>]` | Run the memory HTTP API on loopback as the sync peer for `memory sync --remote` |
| `symbrain skills list \| status \| targets \| log \| sync \| doctor` | Inspect and repair the embedded skill library and its installs |
| `symbrain skills import \| pull \| install \| uninstall \| render \| validate \| history \| restore` | Manage skill content: import and pull into the library, install, uninstall (all targets, one result each, drops the render copy), render per target, validate, and read or restore history. install/status/sync warn `source-dirty` when `library/<name>` has uncommitted git changes (`install --require-clean` refuses) |
| `symbrain activity` | Read bounded activity summaries with explicit profile access |
| `symbrain audit tail [-n N] [--profile <name>] [--json]` | Inspect the local JSONL audit log |
| `symbrain vault` | Human credential create/set/delete commands and symvault passthrough |
| `symbrain guard` | Absorbed symguard commands (decide, scan, doctor, grants, version) |
| `symbrain version` | Print version information |
| `symbrain help` | Show this help message |

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

# Focused and complete root checks
make rust-fast
make rust-check parity-smoke init-differential
```

`make rust-check` covers formatting, checking, Clippy, tests, doc tests,
cargo-audit, cargo-deny and the sealed Go-oracle fixture gate. It is not a
substitute for the browse, Swift, GUI and native-platform workflows under
`.github/workflows/`.

---

*This file is a living document. Update it when conventions change.*
