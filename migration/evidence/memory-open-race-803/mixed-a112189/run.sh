#!/usr/bin/env bash
set -euo pipefail
umask 022
source /home/agent/.cargo/env
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH
export HOME=/tmp/symaira-memory803-mixed-a112189/home
export USERPROFILE="$HOME"
export XDG_CONFIG_HOME=/tmp/symaira-memory803-mixed-a112189/config
export XDG_DATA_HOME=/tmp/symaira-memory803-mixed-a112189/data
export XDG_CACHE_HOME=/tmp/symaira-memory803-mixed-a112189/cache
export XDG_STATE_HOME=/tmp/symaira-memory803-mixed-a112189/state
export CARGO_HOME=/home/agent/.cargo RUSTUP_HOME=/home/agent/.rustup
export CARGO_TARGET_DIR=/workspace/symaira-memory803-open/target
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
stage_guard() {
  python3 - <<'PY'
import shutil
free=shutil.disk_usage('/workspace').free
print('available_bytes='+str(free))
assert free >= 700*1024*1024, 'stage stopped below700MiB'
PY
}
stage_guard | tee /tmp/symaira-memory803-mixed-a112189/disk-before.log
rustc -Vv > /tmp/symaira-memory803-mixed-a112189/rust-sdk.log
cargo -V > /tmp/symaira-memory803-mixed-a112189/cargo-sdk.log
go version > /tmp/symaira-memory803-mixed-a112189/go-sdk.log
cargo test -p symbrain-memory --lib --all-features --locked early_writer_then_waiting_reader_share_the_actual_sqlite_timeout -- --nocapture 2>&1 | tee /tmp/symaira-memory803-mixed-a112189/mixed.log
