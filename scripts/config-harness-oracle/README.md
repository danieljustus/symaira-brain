`run.sh OUTPUT_JSON [--record-fixture PATH] [--record-parse-fixture PATH]` compiles the production Go CLI from
immutable `0541e64a36d83bac8e5e4f8c4136033c03db83ae` (the #769 base, with CoreKit v0.18.2,
as for the setup-repair, setup-source and doctor-repair oracles), compiles the candidate Rust CLI, and replays 1931 cases in
disposable per-case HOME/XDG/project roots:

* 116 new argv-grammar shapes for `config`, `harness`, `harness list` and
  `harness health` (help spellings, undefined and bad-syntax flags, bare dash,
  `--` terminator, empty and repeated values, flags before verbs, raw
  non-UTF-8 bytes);
* 34 seeded harness cases (`cfgh-seeded-health-*`, `cfgh-seeded-list-*`): a
  `~/.claude.json` with a stdio server whose command is missing and an HTTP
  server on closed loopback port 1, plus a project `.mcp.json`, replayed with
  empty/repeated/matching/non-matching `--harness` and absolute and relative
  (`.`, `./`, `./x/../`) `--project` values. Probes fail fast and offline; each
  run must finish within 8 s. `harness list` is byte-exact; `harness health`
  server rows (harness, server, config path, unhealthy status, order) must be
  identical and only the diagnostics follow the HAR-007 redaction
  (`scripts/harness_health_contract.py`); no-server output is byte-exact;
* 616 `cfgh-parse-*` cases (#769 slice 2, inventory item D1; corpus in
  `parse_cases.py`), each replayed as `harness list` (table) and
  `harness list --json`: 108 JSON configs (`encoding/json` Decoder syntax errors
  (messages carry no byte offsets) such as `invalid character 'x' looking for beginning
  of value`, `EOF`, `unexpected EOF`, `expected {, got ...`, trailing data, BOM,
  invalid UTF-8, surrogates, 20,000-deep nesting, across claude, claude-desktop,
  cursor, opencode and antigravity), 174 codex TOML configs (BurntSushi/toml
  v1.6.0 lexer, parser, number, datetime and table-conflict errors with
  `toml: line N (last key "k"): ...`), 5 multi-file cases (several broken
  configs at once in registry order, plus a broken project `.mcp.json`) and 21
  POSIX-only read-error cases (`open PATH: <errno>`, `open configuration directory
  component "x": ...`, non-regular files, the 8 MiB limit and its boundary,
  symlinked and unreadable files and parents, symlinked/file/`..` project roots).
  Go decides every expected message; `--record-parse-fixture` rewrites
  `rust/symbrain-harness/tests/fixtures/go_parse_errors.json` from the Go
  observations of the single-file cases (inputs over 4 KiB are skipped) and the cargo test
  `go_parse_errors` replays it without a Go toolchain;
* 174 existing `config_*` / `harness_*` cases from `scripts/rust-differential.py`.
  `harness_health_initialize` and `harness_health_initialize_json` are excluded
  because they use `SYMBRAIN_GO_BINARY` as their fake MCP server; the main
  differential still replays them.

Every Rust process runs with an absent `SYMBRAIN_GO_BINARY`, so any shape that
still routes to Go fails. stdout, stderr and exit codes must match; mutating
cases also compare the full fixture filesystem with modes. Only the fixture root
is normalized (plus the documented HAR-007 health contract for legacy cases).
On Windows, POSIX-only and raw-byte-argv cases are skipped by the shared harness
and listed explicitly in its `SKIP` output and in the report's `skipped` map.

`--record-fixture` rewrites
`rust/symbrain-cli/tests/fixtures/config_harness_go_shapes.json` from the Go
observations (only cases whose output contains no fixture-root path). The
cargo test `config_harness_native` replays that file without a Go toolchain.

Negative controls (each run on the committed tree with one production line
mutated, then restored): `flag provided but not defined` -> `flag provided but
undefined` in `rust/symbrain-cli/src/go_flags.rs` fails 22 cases (21 new + 1
legacy `harness_list_unknown_flag`) and the cargo fixture test; dropping the
empty-`--harness` filter in `harness_cli.rs` fails 10 seeded health cases and
`empty_harness_filter_probes_every_server_like_go`; dropping the lexical clean in
`Harness::project_config_path` fails 10 seeded cases and
`relative_project_config_path_is_cleaned_like_go`. Reports are kept under
`migration/evidence/config-harness-769/`.
The oracle is wired into the `rust-migration`, `rust-migration-macos` and
`rust-migration-windows` jobs of `.github/workflows/ci.yml`; exact-head native
Linux/macOS/Windows runs are still pending, so no three-OS claim is made.

#### `--project` path shapes for install, uninstall and sync (#769 slice 3)

`project_cases.py` adds 975 `cfgh-project-*` cases (plus the 16 target cases below): 78 shapes
(`.`, `./`, `./x/../`, `../project`, `..`, `a/b/../../..`, `a//b`, `a/./b`,
trailing slashes, missing and deep directories, symlinked directories,
symlink chains/loops/dangling links, a symlink inside the path, a file or a
symlink to a file as the project, an unreadable directory, a 300-byte
component, spaces, quotes, newlines, `<&>`, U+2028/U+2029, `~`, `-dash`,
absolute variants of the same, raw non-UTF-8 bytes, the empty value) and 7
`--project=VALUE` shapes, each replayed through up to 12 commands: `install`
and `install --dry-run` on an absent and a pre-seeded `.mcp.json`, `uninstall`
(absent, seeded, `--dry-run`), and `sync` (plain, seeded, `--dry-run`,
`--dry-run --json`, absent and seeded). Seeding writes `.mcp.json`,
`CLAUDE.md` and `AGENTS.md`. All run in the disposable project cwd, are
POSIX-only (`posix_only`: skipped on Windows) and compare stdout, stderr, exit
and the full filesystem manifest with modes and symlink targets.
`NOT_REPLAYED` lists the only exclusions: install/uninstall writes into a
read-only directory (Go names a random temp file or wraps backups in
`harness: reserve backup`).
`--record-project-fixture PATH` writes the root-independent cases with Go's
filesystem effects to
`rust/symbrain-cli/tests/fixtures/project_path_go_shapes.<os>.json`, one per OS
(`darwin`, `linux`): the result is filesystem dependent (APFS refuses to create
non-UTF-8 names with `EILSEQ`, ext4 accepts them), so each fixture is recorded
by the real Go CLI on its own OS and `project_path_native` replays the one for
the host (macOS: darwin, Linux: linux; other unix fails to compile) without Go.
Both hold the same 820 case names. Non-UTF-8 manifest names are stored with
their raw bytes (a fifth `rel_hex` element). The Linux fixture is recorded as
a non-root user (uid 1000, like the CI runner; root ignores the unreadable-
directory shapes) inside Docker, with the Go oracle built in
`golang:1.26` and the Rust binary in `rust:1.98-bookworm`:
`python3 scripts/config-harness-oracle/replay.py symbrain-go symbrain-rust
report.json --record-project-fixture project_path_go_shapes.linux.json`.

#### Existing sync instruction targets (#909)

The 16 POSIX-only `cfgh-sync-target-*` cases cover `CLAUDE.md` as a symlink
(to existing notes), dangling symlink, directory and FIFO, each in normal and
`--dry-run`, table and JSON modes. All must exit 1 with Go's exact read error;
filesystem comparison includes FIFOs without opening them. The native
`project_path_native` sibling test replays `fixtures/sync_target_go_shapes.json`
and bounds each child to 8 seconds so a blocking FIFO regression fails.
This shared fixture was recorded on macOS: these cases use Go's own
platform-independent non-regular-file diagnostic, not OS errno text. Linux
execution remains a separate CI gate.

Record only these additions, without changing either existing OS fixture or
the pinned Go revision/toolchain, using the same replay/recording machinery:

```sh
python3 scripts/config-harness-oracle/replay.py symbrain-go symbrain-rust report.json \
  --case-prefix cfgh-sync-target- \
  --record-project-fixture rust/symbrain-cli/tests/fixtures/sync_target_go_shapes.json
python3 -m unittest discover -s scripts/config-harness-oracle
```

`--case-prefix` fails if it selects no cases and reports the applied filter;
it is for focused regression runs/recording, not a replacement for the full
unfiltered oracle gate.
