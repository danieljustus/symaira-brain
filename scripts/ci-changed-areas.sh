#!/usr/bin/env bash
#
# Maps changed paths (one per line on stdin, repository-relative) to the
# GitHub-hosted macOS jobs they can affect in .github/workflows/ci.yml and
# prints `native=true|false` and `gui=true|false` for $GITHUB_OUTPUT.
#
#   native: rust-migration-macos, rust-mcp-fuzz-smoke, rust-init-native
#   gui:    gui (Xcode build of the Swift app and SymBrainCore tests)
#
# Fail-closed by design: a path this table does not list as inert or
# native-only sets both flags, and an empty change set sets both. Only paths
# listed as inert may skip a job. An inert path whose file name or path is
# referenced by a tracked non-documentation file counts as code, because
# tests and generators read some documents. Add new source areas here when
# you add them, and extend test-ci-changed-areas.sh with a case for each.
set -euo pipefail

native=false
gui=false
seen=false

# Documentation and metadata nothing builds or reads, unless referenced.
referenced() {
  local path=$1 name
  name=${path##*/}
  git grep -q -F -e "$path" -e "$name" -- . \
    ':(exclude)*.md' ':(exclude)docs/**' \
    ':(exclude)scripts/ci-changed-areas.sh' ':(exclude)scripts/test-ci-changed-areas.sh' \
    2>/dev/null
}

while IFS= read -r path || [ -n "$path" ]; do
  [ -n "$path" ] || continue
  seen=true
  case "$path" in
    docs/* | *.md | LICENSE | .gitignore | .github/dependabot.yml | .github/CODEOWNERS | \
    .github/ISSUE_TEMPLATE/* | .github/workflows/codeql.yml)
      if referenced "$path"; then native=true; gui=true; fi ;;
    # Scripts that the gui job or the Swift app build reads, and this table.
    scripts/ci-changed-areas.sh | scripts/test-ci-changed-areas.sh | \
    scripts/check-app-icons.sh | scripts/build-module-packages.sh | \
    scripts/create-symaira-dmg.sh | *.swift)
      native=true; gui=true ;;
    # Native-only: Go, Rust, the migration oracles and other scripts. The gui
    # job never reads these, so only native jobs run for them.
    *.go | go.mod | go.sum | Makefile | Cargo.toml | Cargo.lock | rust-toolchain.toml | \
    deny.toml | cmd/* | internal/* | rust/* | migration/* | browse/* | operate/* | \
    scope/* | guard/* | fuzz/* | scripts/*)
      native=true ;;
    # Everything else, including the CI wiring, this table, its test, the
    # Swift sources and project files, assets and unknown paths, runs all jobs.
    *) native=true; gui=true ;;
  esac
done

# An empty change set cannot prove that nothing changed.
if [ "$seen" = false ]; then
  native=true
  gui=true
fi

printf 'native=%s\ngui=%s\n' "$native" "$gui"
