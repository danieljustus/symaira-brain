# symaira-brain #769 — Go-routing inventory (slice 1)

Base `0541e64a36d83bac8e5e4f8c4136033c03db83ae` (origin/main), worktree
`symaira-brain/.worktrees/night-769`, branch `issue/769-guard-fallbacks-slice1`.
Method: every `None`/`return None` in `rust/symbrain-cli/src` (and
`rust/symguard-cli/src/lib.rs`) was traced to `run_native -> None ->
run_go_fallback` (lib.rs `run_with_executor` / `run_stdio`), not grepped.
Line numbers are for the base revision unless marked *(after)*.
Go oracle ref for all rows: `0541e64a36d83bac8e5e4f8c4136033c03db83ae`
(`git diff --quiet <ref> HEAD -- cmd/symbrain internal` exits 0 at base).

Real Go-routing reaches Go only via `run_go_fallback` (lib.rs:318), which is
entered when `run_native` returns `None`. Exactly these producers exist:

## A. Dispatcher arms in `lib.rs` (`run_native`)

| # | lib.rs | argv shapes routed to Go | Go behaviour preserved | existing oracle |
|---|---|---|---|---|
| A1 | :141 `setup if setup_cli::requires_go_fallback` (setup_cli.rs:23-41) | parsed `setup`/`setup --fix`/`--from-source ...` when the managed bin dir exists and (`--fix`\|source) combined with an invalid/unreadable global config (`vault_config::valid_configuration()` false) or `enabled_cores()` error | `cmd/symbrain/cmd_setup.go:31` cmdSetup config-load failure boundary (error text of the Go config loader) | SET-002 `scripts/setup-repair-oracle` (native repair cases; the Go-routed config-failure boundary is not byte-compared there) |
| A2 | :144 `doctor if doctor_cli::requires_go_fallback` (doctor_cli.rs:32-37) | `doctor --fix` with managed bin dir and invalid config / `enabled_cores()` error | `cmd/symbrain/cmd_doctor.go:201` cmdDoctorWithFormat + config-load diagnostics | DOC-001 `rust-differential.py doctor_*` (native states only) |
| A3 | :153 `usage if usage_cli::requires_go_fallback` (usage_cli.rs:52, `Invocation::requires_go_fallback` :66 = `Report && symbrain_usage::needs_go_fallback()`) | `usage` report when a provider needs a not-yet-native transport | `cmd/symbrain/cmd_usage.go:24` | USE-001 `scripts/usage-*-oracle` |
| A4 | :157 `sync if sync_cli::requires_go_fallback` (sync_cli.rs:60) | **none** — predicate is `false`; dead arm | `cmd/symbrain/cmd_sync.go:23` | `sync_native.rs`, rust-differential `sync_*` |
| A5 | :159 `memory if memory_cli::requires_go_fallback` (memory_cli/routing.rs:5-40+) | `memory serve`, `memory sync <args>`, and further list/rules/query-log/search/set/delete shapes selected by `routing.rs` (read that file for the exact residual predicate; not re-derived here) | `cmd/symbrain/cmd_memory*.go` | MEM-CLI-001/002, MEM-HTTP-001 |
| A6 | :137 `"config" => config_cli::run` returned `Option` (**ported this slice**) | see C | `cmd/symbrain/cmd_config.go:26` | CLI-005A..E (bytes proven with the Go fallback present, see C) |
| A7 | :155 `"harness" => harness_cli::run` returns `Option` | see D | `cmd_harness.go:23`, `cmd_harness_health.go:38` | HAR-006/007, rust-differential `harness_*` |
| A8 | :163 `"skills" => skills_cli::run` returns `Option` | out of scope for #769 slice 1 (skills_cli.rs:549-560 `requires_go_fallback`, :585 `run_list`) | `cmd_skills.go:25..487` | SKL-002..008 |
| A9 | :166 `"guard" => guard_cli::run` returns `Option` | **unreachable None**: `symguard_cli::run` (rust/symguard-cli/src/lib.rs:41-72) only yields `None` through `args.first()?` after an `is_empty` early return. Fully native since #770 (SEC-007). Candidate for a signature cleanup (`-> u8`) in a symguard-cli change; not routing. | `cmd/symbrain/cmd_guard.go` | `guard/scripts/guard-standalone-oracle` |

## B. Candidates the issue names that do NOT route to Go (traced, verified)

* `audit_cli.rs:36,41,50` — inside `parse_audit_command -> Option<(String,&[OsString])>`; caller returns `exit::USAGE`. `audit_cli::run` returns `u8`. Not Go routing. Oracle: AUD-001/CLI-008, `rust-differential.py audit_*`.
* `install_cli.rs:101,110,249,255,264,273,293` — argument parsers / `resolve_target`; `run_install`/`run_uninstall` return `u8`. Not Go routing. Oracle: CLI-009/010 `scripts/install-oracle`.
* `mcp_cli.rs:95,99,462,479` — config/override lookups; `mcp_cli::run`/`run_serve` return `u8`. Not Go routing. Oracle: MCP-005 `mcp_cli_tests.rs`.
* `passthrough.rs:73,77,81,189` — vault binary discovery (`admin_binary`, `path_lookup_in`) for `symvault`, not the Go symbrain. Oracle: VLT-002.
* `guard_cli.rs` is a re-export of `symguard_cli::run` (A9).

## C. `config` (`config_cli.rs`) — **ported natively in this slice**

Base: `config_cli.rs:74 _ => None` (first non-`--` argument starting with `-`).
Routed shapes at base (all measured with the Go fallback removed; all exit 1
`start Go fallback` before the change):

* `config -h|--h|-help|--help|-help=1` — Go prints only the usage (flag.ErrHelp, `fs.Usage = printConfigUsage`) to stderr, exit 2 (`cmd_config.go:26-37`).
* `config -x|--x|---x|-x=1|--x=1|-json path|--json path|--output json path` — `flag provided but not defined: -x` + usage, exit 2.
* `config ----x|--=x|-=x` — `bad flag syntax: <normalized token>` + usage, exit 2.
* `config -` — bare dash is positional: `unknown subcommand "-"`.
* `config -- <token starting with ->` (`-- -x`, `-- --x`, `-- -`) — terminator then `unknown subcommand`.
* `config -x path`, `config -h path`, any flag before the subcommand.
* Non-UTF-8 subcommand: base native arm used Rust `{:?}`; Go uses `%q` (byte-preserving). Now `format_go_quoted`.

Go source: `cmd/symbrain/cmd_config.go:26-47` (FlagSet "config", ContinueOnError, no flags), `cmd/symbrain/flags.go:13-34` normalizeFlags, Go stdlib `flag.(*FlagSet).parseOne`.
Existing coverage: CLI-005A..E (`rust-differential.py` `config_*` cases) but those run with `SYMBRAIN_GO_BINARY` set, so the two `*_fallback` cases (`config_unrelated_subcommand_fallback`, `config_no_subcommand_fallback`) never proved a flag shape native.

## D. `harness` (`harness_cli.rs`)

| # | base line | shape | Go behaviour | status |
|---|---|---|---|---|
| D1 | :137 `return None` after `list()` (inventory error filter `go_json_inventory_error` :152) | `harness list [--json]` where the inventory reports any `config.error` other than the frozen JSON message (`... invalid character 'n'`): other `encoding/json` syntax errors, every TOML (codex `config.toml`) parse error, and path errors such as project paths with `..` components (`configuration directory contains parent component`) or non-directories | `internal/harness/ordered_map.go:77` `parse json: %w` of `encoding/json` Decoder.Token errors; `internal/harness/toml_document.go:23` `parse toml: %w` of BurntSushi/toml errors; wrapped by `document.go:84`; Go's path handling accepts `..` components and non-directory project paths | **still routed to Go** (see remaining): requires reproducing Go's `encoding/json` syntax-error catalogue, BurntSushi/toml error text and Go's project-path inventory behaviour |
| D2 | :251 `probe_results?` (thread join) | `harness health` after a probe worker panic | Go has no `recover` here: an unrecovered probe panic aborts the Go process with exit 2 and a goroutine trace | **intentional divergence (HAR-008)**: Rust deliberately exits 1 with the fixed message `symbrain harness health: probe worker panicked` and never re-runs Go. Covered by unit tests through the `run_probes` seam in `harness_cli.rs` (one and several panicking workers); not byte-compared against Go |
| — | not `None`, but native arms that DIVERGED from Go (found while tracing; measured against real Go before the change) | `harness -h`/`--help`/`help` (Rust: stdout + exit 0; Go: usage on stderr, exit 2, `cmd_harness.go:23-37`); `harness list --` / `-- x`, `list -`, `list --=x`, `list -h|-help|--help` (Rust printed "flag provided but not defined: -h"), `list --project=` (empty value became `./.mcp.json` project; Go treats "" as none); `health -`, `health --=x`; after independent review: `health -harness=` / `-harness ''` (Rust filtered out every server, Go's `name != "" && item.Name != name` treats "" as no filter) and relative `-project .|./|./x/../` (Rust displayed `./.mcp.json`, Go's `filepath.Join` prints `.mcp.json`) | Go `flag.FlagSet.Parse` semantics (`cmd_harness.go:39-60`, `cmd_harness_health.go:38-50`), `internal/harness/probeHarnessHealth`, `filepath.Join` | **fixed** by the shared Go flag scanner, the empty-value filter and a lexical `filepath.Clean` of the project config path; proven by 34 seeded oracle cases (HOME with a stdio and an HTTP server, project `.mcp.json`) |

Existing coverage: HAR-006/007, `harness_*` in rust-differential (never exercised the shapes above).

## E. Other `None` sources, unchanged

`lib.rs` generic `_ =>` arm is native (unknown command). `run_stdio` falls to
`InheritedProcessExecutor` only for the `None`s above.

## Remaining after slice 1 (not done)

1. D1 `harness list` still routes to Go for any inventory `config.error` other than the frozen JSON message, including project paths with `..` components or non-directories (JSON Decoder syntax errors; TOML errors; path errors).
   Related open residuals (not routing, recorded in HAR-008): `install`/`uninstall`/`sync` with a `..`-bearing `--project` (`./x/../`, `../project`) still differ from Go (Rust rejects parent components in the trusted root; measured, pre-existing); a non-UTF-8 `-project` is displayed lossily (U+FFFD) where Go prints the raw bytes (Unix only; Windows argv cannot carry raw bytes).
2. A1–A3, A5 module predicates (setup/doctor config-failure boundary, usage report transports, memory serve/sync/etc.).
3. A8 skills (out of scope here).
4. A9 `symguard_cli::run` `Option` -> `u8` signature cleanup.
5. `git grep "=> None\|return None"` acceptance of #769 is still not met: A1–A5, A7/D1, A8 remain.
