# CLI additions: commands the Go oracle never had

Same pattern as `symaira-desktop/docs/rust-port/cli-additions.md`, sized for
Brain's plain-const usage text.

## Rule

- The Go recordings are history and are never edited, regenerated or re-sealed:
  `rust/symbrain-cli/tests/fixtures/cli_tree_expectations.json` (group
  `cli-oracle`) and `rust_differential_go.{darwin,linux}.json`. The `USAGE` const
  in `rust/symbrain-cli/src/lib.rs` stays the byte-exact Go text.
- New top-level commands are declared in one reviewed file next to the product
  code, `rust/symbrain-cli/assets/cli-additions.json`. The usage the binary prints
  is `frozen USAGE + declared additions` (`src/cli_additions.rs`): each addition
  inserts one line at the end of the first command group (after `guard`), in
  declaration order, name column 12 wide like the recorded lines.
- Tests and `scripts/rust-differential.py` compare the product with the recording
  *through* the same declared additions, so drift fails both ways. Everything the
  additions do not touch stays byte-strict.

## What an addition declares

`name` (1-11 chars, `[a-z][a-z0-9-]*`), `short` (one line), `platform`
(`any` or `macos`). Platform decision: the line is **always listed** and a
`macos` addition gets ` (macOS only)` appended. The commands exist on every
platform (`operate`/`scope` answer "unavailable on this platform", exit 1, off
macOS), so one help text everywhere, no `cfg`, no cfg-shrunk tables (nothing for
`clippy::single_element_loop` to find), and the Linux/macOS parity fixtures stay
identical. A name already in the recorded list, a duplicate or a malformed entry
makes `usage()` panic on first use; the unit tests fail first.

Declared today: `browse`, `operate` (macOS), `scope` (macOS): the pass-through
workers in `src/worker_cli.rs`.

## Add a command

1. Implement and dispatch it in `src/lib.rs` (`run_native`).
2. Add the entry to `assets/cli-additions.json`.
3. Add it to `DECLARED` in `tests/cli_additions.rs` (the reviewer's list). If it
   is a `worker_cli::WORKERS` entry, keep `platform` equal to `macos_only`; the
   unit test `declared_commands_and_workers_agree` fails otherwise.
4. Nothing else: the replay tests and the script apply the file themselves.

## Deliberately not covered (reseal-needed.md items 2-4)

- `symbrain setup` usage (`setup_args.rs`, fixture case `go setup --unknown`):
  still mentions `-modules`/`-from-source` as before.
- `symbrain config` usage (`config_cli.rs`, `config_harness_go_shapes.json`).
- The managed manifest entry `symbrowse` (`managed-oracle`, `managed-manifest-testdata`).
- Sub-usages, MCP catalogs, hidden commands, aliases, reordering or rewording of
  recorded lines: an addition only inserts a line.
- `migration/cli-tree-inventory.md` quotes the Go usage and stays a historical record.

## Test changes

- `tests/cli_tree_tests.rs`, `cli_tree_fixture_matches_native_binary`: expected
  stdout/stderr is `with_additions(frozen text)`. A pre-pass asserts exactly six
  cases are rewritten (`go --help`, `go -h`, `go help`, `go`, `go bogus`,
  `go --unknown`); the other 82 are compared exactly as before. The test applies
  the file with its own anchor replacement (`tests/common/cli_additions.rs`), not
  the product code. Comparison strictness is unchanged.
- `src/cli_tests.rs` (`help_matches_frozen_usage`,
  `missing_command_prints_help_and_exits_usage`,
  `json_help_exits_usage_with_unknown_command`): compare with
  `cli_additions::usage()` instead of the bare const; the frozen text itself is
  pinned by the process test above.
- `scripts/rust-differential.py` replay: the sealed Go stdout/stderr of the usage
  cases (`help`, `help_long`, `help_short`, `no_args`, `json_help`,
  `unknown_command_quoted`, `unknown_fallback`) get the additions applied before
  the byte comparison; the run fails unless exactly these seven were rewritten.
  `scripts/test_rust_differential.py` covers the helper.
- New `tests/cli_additions.rs`: reviewed list; no addition shadows a frozen
  command; the printed usage equals frozen plus additions on every route; each
  declared command is dispatched by the binary (fake worker on `PATH`). Unit
  tests in `src/cli_additions.rs`: rejection of shadowing/malformed entries, and
  `WORKERS` equals the declared commands.
- `make oracle-freeze-check` is unchanged (521 fixtures, 66 groups).
