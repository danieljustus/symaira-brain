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
  `rust-migration-windows` in `.github/workflows/ci.yml`. Native receipt
  (added in slice 3): the slice-1 merge commit `e328ca77e` (PR #901) passed
  main-push CI run 39715748422 (workflow `CI`, conclusion success), with the
  step "Go/Rust native config and harness process parity" green on
  `rust-migration`, `rust-migration-macos` and `rust-migration-windows` and
  `rust-init-native` green on ubuntu-latest, macos-15 and windows-2025.
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
* Native receipt (added in slice 3): the exact PR head `9bf5ed337` (PR #903,
  squash-merged as `1991381c1` with an identical tree) passed the full `CI`
  workflow in pull-request run 39765433476 (conclusion success), including the
  oracle step on `rust-migration`, `rust-migration-macos` and
  `rust-migration-windows` and `rust-init-native` on ubuntu-latest, macos-15
  and windows-2025. Windows-specific read-error wording (Go's
  `GetFileAttributesEx`/`CreateFile` texts) is still not oracle-compared;
  POSIX-only cases are skipped there by the shared harness. The `install`,
  `uninstall` and `sync` residuals recorded in slice 1 were measured and
  closed in slice 3.

## Review fixes: unbounded nesting and dotted keys (`*-deep-nesting-fix.*`)

* Independent review reproduced Rust stack overflows on deeply nested TOML
  (`harness list`, `install`, `uninstall`) and JSON (`install`/`uninstall`) and a
  cubic slowdown for long TOML dotted keys. Fixed in `symbrain-harness`
  (see `MIGRATION-RUST.md`, "Review fixes").
* `oracle-pass-deep-nesting-fix.log` / `oracle-report-deep-nesting-fix.json`:
  `scripts/config-harness-oracle/run.sh` exit 0, **940** parity cases (924 + 8
  new TOML shapes as table and `--json`). The report says `candidate_dirty:
  true` because it was produced before the fix commit existed; the tree was
  byte-identical to the commit apart from `cargo fmt` whitespace in
  `parser.rs`/`json.rs` and the ledger/evidence files.
* `cargo-test-symbrain-cli-harness-core-deep-nesting-fix.log`: `cargo test -p
  symbrain-cli -p symbrain-harness -p symbrain-core --locked` exit 0, 439
  passed. `clippy-workspace-deep-nesting-fix.log`: workspace clippy
  `--all-targets -D warnings` exit 0. `setup-repair-oracle-deep-nesting-fix.log`:
  179 parity cases pass.
* Negative control: with the old `parser.rs` and `json.rs` restored, the
  deep-TOML and deep-JSON cargo tests abort with `has overflowed its stack` and
  the dotted-key test does not finish within 100 s.
* Not re-run after the fix: the 504-case `rust-differential`, profile-remove and
  install oracles (the touched modules are `harness list`/parse paths only; the
  install paths share `json.rs::marshal`, covered by the golden tests in
  `harness_tests.rs`).


# #769 slice 3 evidence (`--project` path shapes for install, uninstall and sync; local macOS arm64, Go 1.26.7)

Candidate: `39b9b4609ea84acbcf8d27059820a449bff50667` (branch
`issue/769-project-path-s3`, base `1991381c1`; code in `fa4d8207b` and
`39b9b4609`, docs and this directory committed afterwards, so every `*-slice3-39b9b46.*`
file was produced from a clean tree, `candidate_dirty` false). Go oracle
unchanged: `0541e64a36d83bac8e5e4f8c4136033c03db83ae`.

* `oracle-report-slice3-39b9b46.json` / `oracle-pass-slice3-39b9b46.log`:
  `scripts/config-harness-oracle/run.sh` exit 0, 1915/1915 executed (940
  earlier + 975 `cfgh-project-*`), `skipped` and `mismatched` empty, Rust
  fallback absent. `--record-project-fixture` re-recorded
  `rust/symbrain-cli/tests/fixtures/project_path_go_shapes.darwin.json` (renamed from
  `project_path_go_shapes.json` afterwards) byte-identically
  (820 cases).
* `pre-fix-project-corpus-baseline-1991381.log`: the same project corpus
  (without the 7 inline shapes, 936 cases) replayed against the unmodified base
  binary: 543 cases differ from Go (55 of 78 shapes). This is the measured
  "before" for the table in `MIGRATION-RUST.md`.
* `negative-control-{symlink-root,sync-clean,json-invalid-byte}-oracle-fail-slice3-fa4d820.log`:
  one-line mutations of the committed tree, filtered replays of the same
  oracle cases (28/48, 5/42, 9/72 failing), restored with `git checkout`; run at
  `fa4d8207b`, the Unix code of `39b9b4609` is identical (only the Windows
  `physical_root` stub differs).
* `cargo-test-symbrain-cli-harness-core-instructions-adapter-slice3-39b9b46.log`
  (471 passed), `clippy-workspace-slice3-39b9b46.log`,
  `setup-repair-oracle-slice3-39b9b46.log` (179 cases),
  `install-profile-remove-oracle-check-slice3-39b9b46.log`,
  `rust-differential-slice3-39b9b46.log` (504 cases),
  `skills-argv-oracle-unittest-slice3-39b9b46.log` (26 tests). `cargo fmt
  --check` and `make lint` exit 0 (not retained as files).
* Windows/Linux: not run locally. The new oracle cases are `posix_only`
  (skipped on Windows); see the residuals in `MIGRATION-RUST.md` (slice 3).

### Linux project-path fixture (CI run 39784227979 / 39784227870 fix)

`project_path_native` replayed the darwin recording on ubuntu and failed
`cfgh-project-072-non-utf8-missing-install: exit left Some(0) right Some(1)`:
APFS refuses to create non-UTF-8 names (`EILSEQ`, Go exits 1), ext4 accepts
them (Go exits 0). Not a Rust divergence: the same Rust build matches the real
Go CLI on Linux.

* New fixture `rust/symbrain-cli/tests/fixtures/project_path_go_shapes.linux.json`
  (820 cases, same names as `project_path_go_shapes.darwin.json`, `go_oracle_ref`
  `0541e64a36d83bac8e5e4f8c4136033c03db83ae`), written by the unmodified recording
  path of `scripts/config-harness-oracle/replay.py --record-project-fixture`
  from the real Go CLI; no expected value was edited by hand. One recorder change:
  a non-UTF-8 manifest name is kept (raw bytes in a fifth `rel_hex` element)
  instead of dropping the case. Darwin entries are unchanged and the test falls
  back to the UTF-8 name for them.
* Recorded at candidate `bcd058028a51ba376b01b2d8466b4933830e0e6f` (local clone,
  plus the `replay.py` change), Docker (linux/arm64), non-root `--user 1000:1000`
  (root bypasses the unreadable-directory shapes and records a different
  fixture; a root run was discarded):
  * Go oracle: `golang:1.26` (`golang@sha256:6c2a5538f964f1c82f97ad14988bf05de100d922d159d0e398b54c7b0ca0c6c9`),
    `GOFLAGS=-buildvcs=false`, `GOTOOLCHAIN=go1.26.7` (go.mod of the oracle ref),
    `CGO_ENABLED=0 go build ./cmd/symbrain` from a detached worktree of the oracle ref.
  * Rust: `rust:1.98-bookworm` (`rust@sha256:93ce27a88655056a51dbdd8f5f2d7ddc071c7b0070fb288a37b5a285fc83971e`),
    `cargo build -p symbrain-cli --bin symbrain --locked`.
  * Recorder: `python3 scripts/config-harness-oracle/replay.py /b/symbrain-go /b/symbrain-rust /b/report.json --record-project-fixture /b/project_path_go_shapes.linux.json`
    in `golang:1.26`, exit 0, `1915 parity cases passed` (Rust equals Go on Linux
    for stdout, stderr, exit and the filesystem manifest for every case;
    `project-path-linux-record-slice3-bcd0580.log`,
    `project-path-linux-record-report-slice3-bcd0580.json`).
* Darwin vs Linux: 14 of 820 cases differ in substance (the non-UTF-8 name
  shapes `072`/`073`/`074`, 4 commands each, and `inline-004`, 2 commands);
  the other recorded differences are symlink own-mode noise, which the test
  already ignores.
* `project_path_native` now selects the fixture by `target_os` (macOS: darwin,
  Linux: linux, other unix: compile error, i.e. unsupported and fail-closed),
  asserts both fixtures have the same oracle ref and case names, and keeps the
  `>= 300` case guard for each. Native checks with an absent Go fallback:
  macOS pass; Linux `rust:1.98-bookworm` as uid 1000 pass
  (`project-path-native-linux-docker-slice3.log`).
* Linux exact-head CI is pending.
* macOS re-check after the recorder change: `scripts/config-harness-oracle/run.sh`
  with `--record-project-fixture` exit 0, 1915/1915
  (`oracle-pass-slice3-bcd0580-darwin.log`), and the darwin fixture re-recorded
  byte-identically. `cargo fmt --all -- --check`, workspace clippy
  `-D warnings`, `cargo test -p symbrain-cli --locked` (41 test binaries ok) and
  `make lint` exit 0.
