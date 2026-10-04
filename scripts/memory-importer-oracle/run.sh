#!/usr/bin/env bash
# PREPARED; never run before Root allocates compiler/cache/target/runtime.
set -euo pipefail
umask 022
: "${IMPORTER761_RUNTIME_ALLOCATED:?explicit Root runtime allocation required}"
: "${CARGO_TARGET_DIR:?existing exclusively allocated target required}"
: "${IMPORTER761_GO_CACHE:?explicitly allocated Go cache required}"
: "${IMPORTER761_WORK_ROOT:?new owned scratch directory required}"
test "$IMPORTER761_RUNTIME_ALLOCATED" = 1
test ! -e "$IMPORTER761_WORK_ROOT"
repo=$(git rev-parse --show-toplevel)
frozen=/workspace/oracles/daemon772-go-source
test "$(git -C "$frozen" rev-parse HEAD)" = dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
mkdir -p "$IMPORTER761_WORK_ROOT/go"
git -C "$frozen" archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c | tar -xf - -C "$IMPORTER761_WORK_ROOT/go"
mkdir -p "$IMPORTER761_WORK_ROOT/go/cmd/owned-importer761"
cp "$repo/scripts/memory-importer-oracle/main.go" "$IMPORTER761_WORK_ROOT/go/cmd/owned-importer761/main.go"
export TZ=UTC GOTOOLCHAIN=local GOCACHE="$IMPORTER761_GO_CACHE" GOPROXY=off GOSUMDB=off CGO_ENABLED=0
export GOMODCACHE=/home/agent/go/pkg/mod
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2
export HOME="$IMPORTER761_WORK_ROOT/home" XDG_CONFIG_HOME="$IMPORTER761_WORK_ROOT/config" XDG_DATA_HOME="$IMPORTER761_WORK_ROOT/data"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME"
python3 "$repo/scripts/memory-importer-oracle/fixtures.py" "$IMPORTER761_WORK_ROOT/fixtures"
python3 "$repo/scripts/memory-importer-oracle/state.py" "$IMPORTER761_WORK_ROOT/fixtures" > "$IMPORTER761_WORK_ROOT/before.json"
(
 cd "$IMPORTER761_WORK_ROOT/go"
 /workspace/toolchains/go1.26.7/bin/go build -trimpath -o "$IMPORTER761_WORK_ROOT/oracle" ./cmd/owned-importer761
)
# rustup/cargo still uses the separately installed toolchain owner's HOME.
(
 export HOME=/home/agent
 cd "$repo"
 /home/agent/.cargo/bin/cargo build --locked --offline -p symbrain-memory --example importers761_oracle
)
"$IMPORTER761_WORK_ROOT/oracle" "$IMPORTER761_WORK_ROOT/fixtures/input.json" > "$IMPORTER761_WORK_ROOT/go.jsonl"
"$CARGO_TARGET_DIR/debug/examples/importers761_oracle" "$IMPORTER761_WORK_ROOT/fixtures/input.json" > "$IMPORTER761_WORK_ROOT/native.jsonl"
python3 "$repo/scripts/memory-importer-oracle/state.py" "$IMPORTER761_WORK_ROOT/fixtures" > "$IMPORTER761_WORK_ROOT/after.json"
cmp "$IMPORTER761_WORK_ROOT/before.json" "$IMPORTER761_WORK_ROOT/after.json"
python3 "$repo/scripts/memory-importer-oracle/compare.py" "$IMPORTER761_WORK_ROOT/fixtures/input.json" "$IMPORTER761_WORK_ROOT/go.jsonl" "$IMPORTER761_WORK_ROOT/native.jsonl"
