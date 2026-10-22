# #769 slice 1 evidence (local macOS arm64, Go 1.26.7 toolchain, no native CI)

Candidate: `8c0eab013078d7ba7a1747e60d18dee60aabc244` (code in `36250aacd`, docs in
`8c0eab013`), branch `issue/769-guard-fallbacks-slice1`. Go oracle:
`0541e64a36d83bac8e5e4f8c4136033c03db83ae` (CoreKit v0.18.2 base). Every log
below was produced from this committed tree, which was clean (`git status
--short` empty) before the runs and again after the negative controls restored
their one-line mutations; this directory is committed in a follow-up commit
that contains evidence files only. Evidence recorded earlier for `de6530b`
(oracle ref `dcddcef0…`) is superseded and was removed; it stays in git history.

* `oracle-report-8c0eab0.json` / `oracle-pass-8c0eab0.log`:
  `scripts/config-harness-oracle/run.sh` exit 0, `candidate_dirty` false,
  `go_oracle_ref` 0541e64a, 324/324 executed (150 new = 116 argv-grammar + 34
  seeded, 174 legacy), `skipped` empty on macOS, `mismatched` empty, Rust
  fallback absent; slowest seeded run 0.013 s (budget 8 s).
* Fixture provenance: `--record-fixture` from the same oracle re-recorded
  `rust/symbrain-cli/tests/fixtures/config_harness_go_shapes.json`
  byte-identically (100 cases, `go_oracle_ref` 0541e64a; seeded cases are not
  recorded because they depend on seeded files).
* Negative controls, each a single-line mutation of the committed tree that was
  restored byte-identically afterwards (tree clean again):
  * `negative-control-flags-*`: `flag provided but not defined` -> `flag
    provided but undefined` in `go_flags.rs`: oracle exit 1, **22** failing
    cases (21 new `cfgh-*` + 1 legacy `harness_list_unknown_flag`; the earlier
    "21" counted only the new ones), 302 pass;
    `go_recorded_shapes_are_native_without_a_go_fallback` fails.
  * `negative-control-f1-*`: drop the empty `--harness` filter (F1): oracle
    exit 1, **10** seeded health cases fail
    (`cfgh-seeded-health-{1,2,3,6,10,11,14,18,20,21}`), 314 pass;
    `empty_harness_filter_probes_every_server_like_go` fails.
  * `negative-control-f2-*`: drop the lexical clean of the project config path
    (F2): oracle exit 1, **10** seeded cases fail (health 14,15,16,21; list
    4,5,6,9,10,11), 314 pass; `relative_project_config_path_is_cleaned_like_go`
    fails.
* `cargo-test-symbrain-cli-harness-8c0eab0.log`: `cargo test -p symbrain-cli
  -p symbrain-harness --locked` exit 0 (43 result lines ok, includes the
  `run_probes` panic tests, the `clean_path`/`project_config_path` unit tests
  and the two native seeded tests). `clippy-workspace-8c0eab0.log`: `cargo
  clippy --workspace --all-targets --locked -- -D warnings` exit 0. `cargo fmt
  --all --check` exit 0 (no output).
* Shared-code check for F2 (`project_config_path` is used by `harness
  list|health`, `install`/`uninstall` and `profile remove`):
  `rust-differential-8c0eab0.log` (504 cases, exit 0),
  `profile-remove-compare-rust-8c0eab0.log` (20 cases),
  `profile-remove-check-8c0eab0.log` and `install-oracle-check-8c0eab0.log`
  (21 cases) all pass.
* Not covered / open (recorded in HAR-008): `harness list` still routes to Go
  for any inventory `config.error` other than the frozen JSON message
  (including cleaned project paths that keep a leading `..` and symlinked or non-directory project roots);
  `install`/`uninstall`/`sync` with a `..`-bearing `--project` (`./x/../`,
  `../project`) differ from Go (measured; pre-existing, not changed here), Rust
  refusing with exit 1 where Go succeeds; `sync --dry-run --json` prints an
  uncleaned `--project` (`a//b`, `a/./b`, `./a/.`) where Go prints it cleaned;
  `install`/`uninstall` refuse a symlinked project root that Go accepts (exit 1
  vs 0, pre-existing); non-directory roots, and `sync` with a symlinked or
  non-directory root, are refused by both with differing stderr text; a
  non-UTF-8 `-project` is displayed lossily (Unix only); the probe-panic exit
  code is an intentional divergence (Go aborts with exit 2 and a trace, Rust
  exits 1), unit-tested but not oracle-compared. The seeded health projection
  (`scripts/harness_health_contract.py`) enforces row identity, order, status
  and the exact redacted diagnostic, but not text-table column padding or JSON
  key order.
* The oracle is wired into `rust-migration`, `rust-migration-macos` and
  `rust-migration-windows` in `.github/workflows/ci.yml`; exact-head native
  CI runs are still pending. This is not evidence of native three-OS CI.
* The pre-change baseline measurements (exit 1 `start Go fallback` for every
  config flag shape, wrong bytes for `harness -h|--help|help`, ...) were taken
  in-session and not retained as a file.


# #769 slice 2 evidence (D1: `harness list` config parse errors; local macOS arm64, Go 1.26.7, no native CI)

Candidate code commit: `9121b8272fbf2def5dfb8359004043eacb7c6d5d` (branch
`issue/769-harness-list-parse-errors`, base `e328ca77e`); the ledger and this
directory are committed afterwards, so every log below was produced from a clean
tree (`candidate_dirty` false in the report). Go oracle unchanged:
`0541e64a36d83bac8e5e4f8c4136033c03db83ae`. Files carry the suffix
`-slice2-9121b82`.

* `oracle-report-slice2-9121b82.json` / `oracle-pass-slice2-9121b82.log`:
  `scripts/config-harness-oracle/run.sh` exit 0, **924/924** executed (324
  slice-1 + 600 new `cfgh-parse-*`: 108 JSON x2, 166 TOML x2, 5 multi-file x2,
  21 POSIX read-error x2), `skipped` empty, `mismatched` empty, Rust fallback
  absent (`SYMBRAIN_GO_BINARY` unset). `go_source_sha256` now also covers
  `internal/safefs` (the Go reader used by `harness list`).
* Provenance: `--record-parse-fixture` re-recorded
  `rust/symbrain-harness/tests/fixtures/go_parse_errors.json` byte-identically
  (271 single-file cases) and `--record-fixture` re-recorded the slice-1
  `config_harness_go_shapes.json` byte-identically. The cargo test
  `go_recorded_parse_results_are_reproduced_without_a_go_binary` replays the
  former without Go.
* Go sources traced: JSON text comes from `decodeJSONObject`
  (`internal/harness/ordered_map.go`, `Decoder.Token`); the TOML text comes
  from `github.com/BurntSushi/toml` v1.6.0 (`go.mod`). Families reproduced with
  the library's exact wording: EOF / `unexpected EOF`, `expected {, got X`,
  `invalid character X looking for beginning of value|object key string|in
  literal|in string literal|in string escape code|in \u hexadecimal character
  escape|after object key|after object key:value pair|after array element|in
  numeric literal|after decimal point...|in exponent...`, trailing content,
  UTF-8 BOM / UTF-16 BOM / invalid UTF-8 byte quoting, `%q`-style rune quoting;
  TOML `line N (last key "k")` prefix, lexer errors (newlines in strings, bad
  escapes, hex-digit counts, surrogates/range, control characters, NUL/UTF-16
  detection, invalid UTF-8 byte), parser errors (blank key, key separators,
  top-level item end, array/inline-table separators, table-name errors),
  duplicate/hash/array redefinition errors, integer/float/datetime validation
  (`out of range for int64`, leading zeroes, underscores, base prefixes, calendar
  checks), plus `open PATH: errno`, safefs component errors, non-regular files,
  8 MiB limit and symlinked/unreadable/`..`/file project roots.
* Behaviour that differs from slice-1 Rust-only limits: Go has no JSON value
  count, string-size or nesting-depth limit below the 8 MiB read cap, so the old
  `harness_tests` expectations were replaced by Go-accepting ones.
* Negative controls (single mutation on the clean commit, restored byte-
  identically; tree clean afterwards), each failing the oracle and, where the
  mutated code is covered by the recorded fixture, the cargo test:
  * `negative-control-json-token-*`: `looking for beginning of value` ->
    `looking for begining of value` in `go_json.rs`: oracle exit 1, **46**
    failing cases; `parse_error_messages_use_go_wording` and
    `go_recorded_parse_results_are_reproduced_without_a_go_binary` fail.
  * `negative-control-toml-lexer-*`: `strings cannot contain newlines` ->
    `new lines` in `go_toml/lexer.rs`: oracle exit 1, **6** failing cases; the
    two cargo tests fail.
  * `negative-control-io-errno-*`: errno text lower-casing flipped in
    `go_read.rs`: oracle exit 1, **26** failing POSIX read-error cases (the
    cargo fixture holds no read-error shapes, so only the oracle detects it).
* `cargo-test-symbrain-cli-harness-core-slice2-9121b82.log`: `cargo test -p
  symbrain-cli -p symbrain-harness -p symbrain-core --locked --no-fail-fast`
  exit 0, 423 passed. `clippy-workspace-slice2-9121b82.log`: `cargo clippy
  --workspace --all-targets --locked -- -D warnings` exit 0.
  `clippy-harness-x86_64-pc-windows-msvc-slice2-9121b82.log`: cross-target
  clippy of `symbrain-harness` exit 0 (compile check only, not Windows
  runtime evidence). `cargo fmt --check` exit 0. No Go files changed, so
  `make lint` was not required.
* Shared-code regressions (installers and profile remove share the harness
  reader/formatters): `rust-differential-slice2-9121b82.log` (504 cases),
  `profile-remove-compare-rust-slice2-9121b82.log`,
  `profile-remove-check-slice2-9121b82.log` and
  `install-oracle-check-slice2-9121b82.log` all pass.
* Not run: `cargo test --workspace` fails one unrelated test,
  `symbrain-audit raw::tests::rejects_non_regular_targets_without_blocking`,
  which requires `SYMAIRA_EXTERNAL_RUNTIME_ROOT` on local macOS (crate not
  touched); the other 1114 pass.
* Open: exact-head native Linux/macOS/Windows CI for the 924-case oracle is
  pending (no three-OS claim). Windows-specific read-error wording (Go's
  `GetFileAttributesEx`/`CreateFile` texts) is not oracle-compared locally;
  POSIX-only cases are skipped there by the shared harness. `install`,
  `uninstall` and `sync` residuals recorded in slice 1 were not re-measured.
