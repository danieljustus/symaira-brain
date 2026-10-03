#!/usr/bin/env bash
# Actual immutable Go handlers versus the standalone native Guard.
set -euo pipefail
umask 022
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
report="${1:?usage: scripts/guard-standalone-oracle/run.sh REPORT_JSON}"
owned="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/guard-standalone-770.XXXXXX")"
# Git Bash receives RUNNER_TEMP as a native Windows path; tar needs its MSYS form.
if command -v cygpath >/dev/null 2>&1; then
  owned="$(cygpath -u "$owned")"
fi
trap 'rm -rf "$owned"' EXIT
mkdir -p "$owned/go-source/oracle770"
git archive "$oracle_ref" | tar -xf - -C "$owned/go-source"
cp scripts/guard-standalone-oracle/main.go.txt "$owned/go-source/oracle770/main.go"
go_binary="$owned/symguard-go"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/symguard"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "$rust_binary.exe")"
fi
(cd "$owned/go-source" && GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0 go build -mod=readonly -o "$go_binary" ./oracle770)
cargo build --locked -p symguard-cli
python3 scripts/guard-standalone-oracle/replay.py "$go_binary" "$rust_binary" "$report" "$owned/go-source"
python3 scripts/guard-standalone-oracle/controls.py "$go_binary" "$rust_binary" "${report%.json}-controls.json"
python3 scripts/guard-standalone-oracle/raw_paths.py "$go_binary" "$rust_binary" "${report%.json}-raw-paths.json"
