`run.sh OUTPUT_JSON [--record-fixture PATH]` compiles the production Go CLI from
immutable `dcddcef0df5789123c7c9a7ebe6e01f10e941f2c` (the revision shared by the
sibling replay oracles; `cmd/symbrain` and `internal/` are byte-identical to it
at the #769 base), compiles the candidate Rust CLI, and replays 290 cases in
disposable per-case HOME/XDG/project roots:

* 116 new argv-grammar shapes for `config`, `harness`, `harness list` and
  `harness health` (help spellings, undefined and bad-syntax flags, bare dash,
  `--` terminator, empty and repeated values, flags before verbs, raw
  non-UTF-8 bytes);
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

Negative control: mutating one diagnostic string in
`rust/symbrain-cli/src/go_flags.rs` (`flag provided but not defined` ->
`flag provided but undefined`) makes this oracle fail 21 cases and the cargo
fixture test fail. Reports are kept under `migration/evidence/config-harness-769/`.
This oracle is not yet wired into CI; exact-head native Linux/macOS/Windows runs
are required before any three-OS claim.
