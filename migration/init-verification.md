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

At this checkpoint native Linux/Windows runtime is pending. Do not promote CLI-006A to all-platform verified until exact-head native CI reports and raw observations have been checked. Value and full-product rollback acceptance remain separate repository gates.
