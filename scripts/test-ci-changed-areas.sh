#!/usr/bin/env bash
#
# Table test for scripts/ci-changed-areas.sh, run by the `changes` job in
# .github/workflows/ci.yml before it selects macOS jobs. Runs in a throwaway
# git repository so the "referenced" check sees real tracked files.
set -euo pipefail

script="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/ci-changed-areas.sh"
repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT

cd "$repo"
git init -q
mkdir -p docs/adr internal/x scripts Sources/A
echo "# guide" > docs/adr/guide.md
echo "# unreferenced" > docs/adr/lonely.md
echo "see docs/adr/guide.md" > scripts/uses-doc.sh
echo "see README.md" > scripts/uses-readme.sh
echo "readme" > README.md
echo "package x" > internal/x/x.go
echo "swift" > Sources/A/A.swift
git add -A
git -c user.name=test -c user.email=test@example.invalid commit -q -m fixture

failures=0

# expect "<native> <gui>" <changed path>...
expect() {
  local want="$1"; shift
  local got
  got="$(printf '%s\n' "$@" | "$script" | sed -n 's/^[a-z]*=//p' | tr '\n' ' ' | sed 's/ $//')"
  if [ "$got" != "$want" ]; then
    printf 'FAIL [%s]: want "%s", got "%s"\n' "$*" "$want" "$got" >&2
    failures=$((failures + 1))
  fi
}

# Empty change set and unknown paths cannot skip anything.
expect "true true"
expect "true true" foo/unknown.txt
expect "true true" .github/workflows/ci.yml
expect "true true" .github/workflows/browse-daemon-native.yml
expect "true true" .gitattributes
expect "true true" scripts/ci-changed-areas.sh
expect "true true" scripts/test-ci-changed-areas.sh
expect "true true" scripts/check-app-icons.sh
expect "true true" Sources/A/A.swift
expect "true true" Tests/SymBrainCoreTests/X.swift
expect "true true" project.yml
expect "true true" assets/app-icon/AppIcon.icon/icon.json
expect "true true" SymBrain.xcodeproj/project.pbxproj

# Native-only paths keep native jobs and skip the gui job.
expect "true false" internal/x/x.go
expect "true false" go.mod
expect "true false" Cargo.lock
expect "true false" Makefile
expect "true false" rust/symbrain-cli/src/main.rs
expect "true false" migration/evidence/a.json
expect "true false" scripts/rust-differential.py

# Inert documentation skips every macOS job.
expect "false false" docs/adr/lonely.md
expect "false false" .github/dependabot.yml
expect "false false" LICENSE
expect "false false" docs/adr/lonely.md .github/ISSUE_TEMPLATE/bug.md

# Inert paths that are referenced by code count as code.
expect "true true" docs/adr/guide.md
expect "true true" README.md

# One unknown path in a mixed set forces everything.
expect "true true" docs/adr/lonely.md foo/unknown.txt
expect "true false" docs/adr/lonely.md internal/x/x.go

if [ "$failures" -ne 0 ]; then
  exit 1
fi
echo "ci-changed-areas: all cases passed"
