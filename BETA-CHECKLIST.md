# symbrain v0.1.0 Beta — Manual End-to-End Checklist

Mirrors `ARCHITEKTUR.md` §11 (Erfolgs-Kriterien für v0.1.0). Each item is
checked off with the date, machine and evidence once executed. Items that
require a GUI harness (Claude Code, Cursor) are executed as far as the CLI
allows and marked accordingly.

Environment for the 2026-07-21 run: macOS (darwin/arm64), Go 1.26,
symbrain built from source (`go build ./cmd/symbrain`), real
`symvault`/`symmemory`/`symskills` on PATH.

## 1. Install → init → install --harness claude --profile personal

- [x] `go build -o symbrain ./cmd/symbrain` succeeds, `symbrain version` prints
      the version. (2026-07-21, this machine)
- [x] `symbrain init` creates `~/.config/symbrain/` with config and profile
      scaffold (personal.toml, restricted.toml). (2026-07-21)
- [x] `symbrain install --harness claude --profile personal` writes the Claude
      MCP config entry (`.claude.json`) pointing at `symbrain serve --profile personal`.
      (2026-07-21)
- [ ] Claude Code attaches and sees memory-, vault- and skills-tools through
      the single symbrain server. **Requires GUI harness — to be executed by
      the maintainer before release.**

## 2. Restricted profile on a second harness

- [x] Profile `restricted` with `servers.vault.mode = "request_only"` and
      `servers.memory.mode = "read_only"` loads without warnings.
      (2026-07-21)
- [x] Over `symbrain serve --profile restricted --vault-agent claude-code`
      (real children), `tools/list` exposes `memory_search`/`memory_get`/
      `memory_list` (3 read-only), hides `memory_set`, and vault exposes
      only `vault_generate_password`/`vault_health`/`vault_request_credential`
      (request_only). `vault_get_entry` is hidden.
      (2026-07-21)
- [x] Over `symbrain serve --profile personal --vault-agent claude-code`
      (real children), `tools/list` exposes 25 tools across all three state
      cores (vault=10, memory=4, skills=7). `vault_get_entry` is visible.
      `memory_set` is visible. (2026-07-21)
- [ ] `request_credential` flow completes end-to-end (native dialog +
      vault write). **Interactive — to be executed by the maintainer.**
- [ ] Cursor attaches to the restricted profile. **Requires GUI harness.**

## 3. `symbrain sync` idempotency + symskills

- [x] `symbrain sync` creates AGENTS.md-based instruction targets for
      claude, cursor, gemini; a second consecutive run reports every target
      `unchanged`. (2026-07-21)
- [x] symskills is detected and invoked; its `--json` output is parsed into
      the per-skill summary. (2026-07-21)

## 4. `symbrain doctor` explains degraded states

- [x] With all three binaries on PATH, `symbrain doctor` reports handshake
      success for all three state cores (vault=10, memory=8, skills=7 tools)
      with protocol version. (2026-07-21)
- [x] `symbrain doctor` reports missing binaries as understandable degraded
      states (not stack traces or bare exit codes). (2026-07-21)

## 5. Audit log shows both profiles, zero secrets

- [x] After driving calls through `personal` and `restricted`, the JSONL audit
      log contains entries for both profiles with tool names, durations and
      statuses. (2026-07-21)
- [x] Grep of the audit log for known secret-shaped values (vault entry
      contents, request_credential payloads, argument values of `vault_*`
      calls) finds nothing — argument keys only, never values, and nothing at
      all for `vault_*`. (2026-07-21)

## Findings (fixed as part of this issue)

1. **Child spawn args missing**: `cmd_serve.go` spawned children without
   `serve` subcommand (`symvault serve`, `symmemory serve`, `symskills serve
   --stdio`). Fixed by adding per-server args.
2. **Vault agent flag required**: `symvault serve --stdio` requires
   `--agent <name>` and `--allow-locked` for non-interactive use. Added
   `--vault-agent` flag to `symbrain serve` and `symbrain doctor`.
3. **Audit not wired into gateway**: The audit package was implemented but
   not integrated into the MCP gateway's tool call path. Fixed by adding
   audit logging in `Server.ServeIO`.

## Sign-off

- [ ] All GUI-harness items above executed on a real machine.
- [ ] Checklist reviewed; issue #28 closed via the release PR.
