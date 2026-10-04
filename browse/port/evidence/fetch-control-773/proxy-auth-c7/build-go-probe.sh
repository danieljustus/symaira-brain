#!/usr/bin/env bash
set -euo pipefail
umask 022
owned="$(mktemp -d /tmp/fetch773-auth-oracle.XXXXXX)"
trap 'rm -rf "$owned"' EXIT
git -C /workspace/symaira-fetch773-proxy-auth archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c browse | tar -xf - -C "$owned"
mkdir -p "$owned/browse/port773"
cp /workspace/symaira-fetch773-proxy-auth/browse/port/harness/fetch_control_773.go.txt "$owned/browse/port773/main.go"
cd "$owned/browse"
GOTOOLCHAIN=go1.26.7 CGO_ENABLED=0 go build -mod=readonly -trimpath -o /tmp/symaira-fetch773-proxy-auth-clean-go-probe ./port773
