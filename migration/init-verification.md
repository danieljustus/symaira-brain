# Native init migration evidence

Contract CLI-006A is the bounded `symbrain init` portion of CLI-006. The full command tree and CFG-001 remain open. No release or production cutover is authorized by this slice.

## Source and behavior

The oracle is the unreleased immutable commit `d53c3824e7d6771fabefd62a18dd3c5e50c3c37d`. `scripts/init-oracle/compare.py` extracts its Git archive with LF settings and verifies extracted source bytes against that commit's Git blobs before building the real Go CLI. The public Rust CLI dispatches init in process; its tests also run with the Go fallback unavailable.

The 20-case differential compares exact stdout/stderr, status and filesystem manifests: initial files, existing edits, XDG precedence, missing home, flag failures, positional/terminator parsing, directory collisions, existing/dangling/cyclic symlinks and partial side effects. Templates retain the Go bytes. Creation modes remain 0700/0600 on Unix; Windows uses its native filesystem semantics. The comparator has no output normalization. The original Go implementation remains available for rollback; running Go init after Rust must preserve existing files under the same write-if-missing contract.

## Reproduction and observations

Use the repository's pinned Rust toolchain and Go version from go.mod:

    make init-differential
    cargo test -p symbrain-cli --test init_cli_tests --locked
    cargo clippy -p symbrain-cli --all-targets --all-features --locked -- -D warnings
    make rust-check parity-smoke
    make lint build
    go test -race ./...

Run tests with private HOME/XDG/TMP and explicit toolchain/cache paths, not user data. Native CI job `rust-init-native` executes the real Go comparison on Linux, macOS and Windows and retains `target/init-oracle/report.json` plus Go build logs. Cross-compilation does not prove runtime acceptance.

Local macOS results: all 20 differential cases and the focused init subprocess tests pass. Full Rust/parity, Go lint/build/race gates passed in the preceding verification run. Two actual compiled source mutants (wrong stderr casing and suppressed file writes) each made the same differential comparator fail; restoring exact source bytes made all 20 cases pass again. Evidence is retained under the consumers-night run76/run77 checkpoints, not generated from expected Rust output.

## Three-platform runtime checkpoint (not yet accepted)

Implementation source `79ebab1ba005cefdbbbc17d689f3e6b225235349` was exercised in
[CI run 34941579018](https://github.com/danieljustus/symaira-brain/actions/runs/34941579018).
All three native reports contain 20 distinct passing cases, zero failures,
Go 1.26.7, Rust 1.98.0, executable hashes and identical pinned-Go source manifests.
Linux and macOS init jobs passed. Windows runtime passed four init unit tests,
seven subprocess tests, all 14 comparator controls and the genuine 20-case
differential, but its required Clippy step failed. This is not an accepted gate.

The scoped repairs preserve LF template bytes under Windows checkout, native
path separators, Go Windows missing-home/collision/symlink diagnostics, and
bounded cleanup of the comparator's Windows command-wrapper descendants.
The authorized policy mode helpers retain fallible Unix 0700/0600 operations
and use infallible non-Unix no-ops, without suppressing lint warnings.

At that historical checkpoint, `rust/symbrain-broker/src/client.rs:230`, `Inner.pid`,
was unused on Windows and failed `-D warnings`; its only read is Unix-only.
Subsequent authorized cfg-only repairs are recorded below. Required lint
remains fatal even though the separately observed runtime passes. Tracking:
[issue 601](https://github.com/danieljustus/symaira-brain/issues/601).

At the same implementation source, local `make rust-check` and `make parity-smoke`
passed. The first full run hit an unrelated 25 ms doctor subprocess-startup test
failure; an unchanged rerun passed, and both logs are retained. Focused init and
policy tests, formatting, actionlint and Unix Clippy passed. Actual compiled
wrong-stderr-case and no-file-write mutants failed 1 and 13 differential cases
respectively; exact source restoration passed all 20 cases. Original and restored
init source SHA-256: `8585c62c4e7cbb6bd8ac6d967e6d2b98c29c1eae3562aac665146904180fa43e`.

Raw native reports/logs and source-bound local command manifests are retained in
the central `docs/intern/rust-resume-evidence/consumers-slice-brain-init-*` evidence
lane. A later documentation-only commit is not itself the tested source SHA;
its exact-head workflow outcome must be recorded separately in the task handoff.
## Bounded implementation acceptance; Windows deferred

Source `9620d92903384fbf7d3738e92541108f2a9e2bb2` was exercised in
[CI run 34945784897](https://github.com/danieljustus/symaira-brain/actions/runs/34945784897).
Linux and macOS native init jobs passed formatting, strict Clippy, focused tests,
14 comparator controls and all 20 exact Go-to-Rust cases. Native Windows built
the actual CLI and passed all 20 differential cases, but strict Clippy failed:

- `init_cli.rs:108`: `native_path` triggers `needless_pass_by_value` on Windows.
- `passthrough.rs:265`: Windows-only `return` triggers `needless_return`.

These remain unresolved; passing runtime is not full Windows acceptance.
The operator deferred Windows full acceptance and Windows Clippy until after
functional consumer migration. The bounded implementation milestone can close
with `windows_acceptance=deferred`; the integrator owns the final native Windows
phase before overall completion, release or cutover. No lint threshold, workflow
failure behavior, comparator assertion or required case was weakened.

The completed dependency repairs only scope Unix-only fields/imports/helpers and
discard unused non-Unix bindings. Broker process behavior, pattern storage,
archive integrity, doctor diagnostics and Guard key-file error propagation remain
unchanged. Local `make rust-check parity-smoke` passed against the final source
manifest, including Unix lifecycle/security tests and 299 CLI parity cases.
The exact unchanged init source retains the genuine mutant/restoration evidence
above. Go sources are unchanged; the preceding source-bound Go lint/build/race
evidence remains applicable. Raw failures and native reports remain retained.

A documentation-only successor must record its exact-head workflow separately in
the task handoff; this section identifies the tested implementation source.
CLI-006, CFG-001, value and full-product rollback acceptance remain open.
