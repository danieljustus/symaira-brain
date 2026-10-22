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
