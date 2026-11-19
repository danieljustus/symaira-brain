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
  is `frozen USAGE + declared additions` (`src/cli_additions.rs`): one line per
  addition, inserted after the recorded `guard` line (end of the first command
  group) in declaration order, name column as wide as the recorded lines.
- Tests and `scripts/rust-differential.py` compare the product with the recording
  *through* the same declarations, so drift fails both ways. Everything else stays
  byte-strict.

## What an addition declares

`name` (1-11 chars, `[a-z][a-z0-9-]*`), `short` (one line), `platform` (`any` or
`macos`). Platform decision: always listed; a `macos` entry gets ` (macOS only)`
appended. The commands exist on every platform (`operate`/`scope` print
"unavailable on this platform", exit 1, off macOS), so there is one help text,
no `cfg`, no cfg-shrunk table (nothing for `clippy::single_element_loop`), and the
Linux and macOS parity transcripts get the same rewrite. A name the recording
already lists, a duplicate or a malformed entry makes `usage()` panic on first
use; the unit tests fail first.

Declared today: `browse`, `operate` (macOS), `scope` (macOS), the pass-through
workers of `src/worker_cli.rs`.

## Add a command

1. Implement and dispatch it in `run_native` (`src/lib.rs`).
2. Add the entry to `assets/cli-additions.json`.
3. Add it to `DECLARED` in `tests/cli_additions.rs` (the reviewer's list). A
   `worker_cli::WORKERS` entry must be declared with `platform` equal to
   `macos_only` (unit test `every_worker_command_is_declared_with_its_platform`).
   The test that checks dispatch assumes a worker `sym<name>` on `PATH`; a
   non-worker command needs its own dispatch assertion.
4. Nothing else: the replay tests and the script apply the file themselves.

## Deliberately not covered (reseal-needed.md items 2-4)

- `symbrain setup` usage (`setup_args.rs`, case `go setup --unknown`) and
  `symbrain config` usage (`config_cli.rs`, `config_harness_go_shapes.json`).
- The managed manifest entry `symbrowse` (`managed-oracle`,
  `managed-manifest-testdata`).
- Sub-usages, MCP catalogs, aliases, hidden commands, reordering or rewording
  recorded lines: an addition only inserts a line.
- `migration/cli-tree-inventory.md` quotes the Go usage and stays a record.

## Test changes

- `tests/cli_tree_tests.rs`, `cli_tree_fixture_matches_native_binary`: expected
  stdout/stderr is `with_additions(recorded text)`. A pre-pass asserts exactly six
  cases are rewritten (`go --help`, `-h`, `help`, `go`, `bogus`, `--unknown`); the
  other 82 are compared as before. The rewrite is an independent anchor
  replacement in `tests/common/cli_additions.rs`, not the product code.
- `src/cli_tests.rs`: `help_matches_frozen_usage` (renamed `..._plus_additions`),
  `missing_command_prints_help_and_exits_usage` and
  `json_help_exits_usage_with_unknown_command` compare with
  `cli_additions::usage()` instead of the bare const; the bytes themselves are
  pinned by the process test above.
- `scripts/rust-differential.py` replay: the sealed Go output of `help`,
  `help_long`, `help_short`, `no_args`, `json_help`, `unknown_command_quoted`,
  `unknown_fallback` gets the additions before the byte comparison; a full run
  fails unless exactly these seven were rewritten.
  `scripts/test_rust_differential.py` (`CliAdditionsTests`) covers the helper.
- New `tests/cli_additions.rs`: reviewed list, no shadowing of recorded commands,
  printed usage on every route, dispatch of every declared command (fake worker
  on `PATH`). New unit tests in `src/cli_additions.rs`.
- `make oracle-freeze-check`: unchanged, 521 fixtures in 66 groups.
