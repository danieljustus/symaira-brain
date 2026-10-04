#!/usr/bin/env bash
set -euo pipefail
umask 022
cd /workspace/symaira-daemon772-state-key-osargs
. /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:/workspace/toolchains/bin:$PATH
export CARGO_TARGET_DIR=/workspace/symaira-daemon772-registry/target
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
prefix=/tmp/symaira-state-key-osargs
source_revision=$(git rev-parse HEAD)
test -z "$(git status --porcelain)"
printf '%s\n' "$source_revision" > "$prefix-source.txt"
cd browse
cargo fmt --all -- --check > "$prefix-fmt.log" 2>&1
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings > "$prefix-clippy.log" 2>&1
python3 port/harness/key_test_environment.py -- cargo test -p symbrowse-core -p symbrowse-protocol -p symbrowse-daemon -p symbrowse-cli --all-features --locked > "$prefix-affected.log" 2>&1
python3 port/harness/key_test_environment.py -- cargo test -p symbrowse-mcp --lib --all-features --locked > "$prefix-mcp-lib.log" 2>&1
cargo build -p symbrowse-cli --bin symbrowse --locked > "$prefix-build.log" 2>&1
cargo build -p symbrowse-core --example state_key_store_probe --example startup_key_source_probe --locked >> "$prefix-build.log" 2>&1
cargo clippy -p symbrowse-core --all-targets --all-features --target x86_64-pc-windows-gnu --locked -- -D warnings > "$prefix-windows-static.log" 2>&1
cargo check -p symbrowse-core --all-targets --all-features --target x86_64-apple-darwin --locked > "$prefix-darwin-static.log" 2>&1
cd ..
actionlint .github/workflows/browse-daemon-native.yml > "$prefix-actionlint.log" 2>&1
python3 browse/docs/rust-port/validate.py > "$prefix-matrix.log" 2>&1
python3 browse/port/harness/test_windows_pipe.py > "$prefix-pipe-tests.log" 2>&1
python3 browse/port/harness/test_registry_progress.py > "$prefix-progress-tests.log" 2>&1
python3 browse/port/harness/test_run.py DaemonExitTests CargoTargetRootTests.test_ci_default_target_stays_in_browse_root CargoTargetRootTests.test_ci_report_path_remains_portable CargoTargetRootTests.test_relative_target_is_resolved_against_browse_root CargoTargetRootTests.test_absolute_target_is_preserved > "$prefix-run-portable-tests.log" 2>&1
rust_binary="$CARGO_TARGET_DIR/debug/symbrowse"
go_binary=/tmp/symaira-pr801-go-1.26.7
go_source=/workspace/oracles/daemon772-go-source
python3 browse/port/harness/daemon_state_key_ownership.py --rust "$rust_binary" --rust-source-probe "$CARGO_TARGET_DIR/debug/examples/startup_key_source_probe" --go-tool /workspace/toolchains/go1.26.7/bin/go --out "$prefix-ownership.json" > "$prefix-ownership.log" 2>&1
python3 browse/port/harness/daemon_state_key.py --go "$go_binary" --rust "$rust_binary" --rust-store-probe "$CARGO_TARGET_DIR/debug/examples/state_key_store_probe" --go-source "$go_source" --go-tool /workspace/toolchains/go1.26.7/bin/go --out "$prefix-key.json" > "$prefix-key.log" 2>&1
python3 browse/port/harness/daemon_process.py --go "$go_binary" --rust "$rust_binary" --go-source "$go_source" --out "$prefix-process.json" > "$prefix-process.log" 2>&1
python3 browse/port/harness/daemon_registry.py --go "$go_binary" --rust "$rust_binary" --go-source "$go_source" --out "$prefix-registry.json" > "$prefix-registry.log" 2>&1
python3 browse/port/harness/daemon_mcp.py --go "$go_binary" --go-fixture /tmp/symaira-pr801-go-mcp-v0.8.0 --rust "$rust_binary" --go-source "$go_source" --workspace --out "$prefix-mcp.json" > "$prefix-mcp.log" 2>&1
test "$(git rev-parse HEAD)" = "$source_revision"
test -z "$(git status --porcelain)"
