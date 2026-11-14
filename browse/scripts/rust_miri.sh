#!/usr/bin/env bash
# Scoped Miri run for the pure Browse boundaries (RUST-015).
#
# Only targets that need no filesystem, process, socket, browser or network
# facility run under Miri; the explicit skip list lives in
# docs/rust-port/hardening.md. A broad `--workspace` run is deliberately not a
# substitute, so adding a pure target means adding it here. Run through
# `dev-external` so build output stays outside the repository.
#   MIRI_TOOLCHAIN  nightly toolchain with the miri component (default: nightly)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
toolchain="${MIRI_TOOLCHAIN:-nightly}"

cargo "+$toolchain" miri setup
# Pure property suite: YAML totality, authenticated state round trips,
# state-name path safety, allowlist label boundaries and IP classification.
cargo "+$toolchain" miri test --locked -p symbrowse-core --test hardening_properties
# Pure daemon frame decoding (no sockets): the protocol module unit tests only.
cargo "+$toolchain" miri test --locked -p symbrowse-daemon --lib -- protocol::
