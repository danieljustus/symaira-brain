#!/usr/bin/env bash
# Supplementary immutable-Go process proof; old Go/fixtures remain untouched.
set -euo pipefail
umask 022
report="${1:?usage: run_fetch_control_773.sh REPORT_JSON}"
owned_root="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
# Git Bash receives native Windows paths; mktemp and tar need the MSYS form.
if command -v cygpath >/dev/null 2>&1; then
  owned_root="$(cygpath -u "$owned_root")"
fi
owned="$(mktemp -d "$owned_root/fetch773-source.XXXXXX")"
trap 'rm -rf "$owned"' EXIT
git -C "$(git rev-parse --show-toplevel)" archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c browse | tar -xf - -C "$owned"
mkdir -p "$owned/browse/port773"
cp port/harness/fetch_control_773.go.txt "$owned/browse/port773/main.go"
go_binary="$owned/go-probe"
rust_binary="${CARGO_TARGET_DIR:-target}/debug/examples/control_probe"
go_source="$owned/browse"
if [ "${OS:-}" = Windows_NT ]; then
  go_binary="$(cygpath -m "$go_binary.exe")"
  rust_binary="$(cygpath -m "$rust_binary.exe")"
  go_source="$(cygpath -m "$go_source")"
fi
(cd "$owned/browse" && GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0 go build -mod=readonly -trimpath -o "$go_binary" ./port773)
cargo build --locked -p symbrowse-fetch --example control_probe
python3 port/harness/fetch_control_process.py --go "$go_binary" --rust "$rust_binary" \
  --go-source "$go_source" --output "$report"
python3 port/harness/fetch_control_negative.py --go "$go_binary" --rust "$rust_binary" \
  --go-source "$go_source" --output "${report%.json}-controls.json"
python3 port/harness/fetch_control_raw_proxy.py --go "$go_binary" --rust "$rust_binary" \
  --output "${report%.json}-raw-proxy.json"
python3 port/harness/fetch_control_raw_proxy_negative.py --go "$go_binary" --rust "$rust_binary" \
  --output "${report%.json}-raw-proxy-controls.json"
python3 port/harness/fetch_control_proxy_auth.py --go "$go_binary" --rust "$rust_binary" \
  --output "${report%.json}-proxy-auth.json"
python3 port/harness/fetch_control_proxy_auth_negative.py --go "$go_binary" --rust "$rust_binary" \
  --output "${report%.json}-proxy-auth-controls.json"
