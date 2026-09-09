#!/usr/bin/env bash
# Run a Go oracle command from an immutable Git revision in an isolated worktree.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <git-revision> <go-arguments...>" >&2
  exit 2
fi

revision=$1
shift
repo_root=$(git rev-parse --show-toplevel)
commit=$(git -C "$repo_root" rev-parse --verify "${revision}^{commit}")
temp_root=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-go-oracle.XXXXXX")
source_root="$temp_root/source"
cleanup() {
  git -C "$repo_root" worktree remove --force "$source_root" >/dev/null 2>&1 || true
  rm -rf "$temp_root"
}
trap cleanup EXIT INT TERM

# A worktree, instead of an archive, preserves Git history required by the
# instruction oracle while still isolating execution from dirty candidate files.
git -C "$repo_root" worktree add --quiet --detach "$source_root" "$commit"
toolchain=$(awk '$1 == "go" { print "go" $2; exit }' "$source_root/go.mod")
if [[ -z "$toolchain" ]]; then
  echo "cannot determine Go toolchain from oracle revision $commit" >&2
  exit 1
fi

GOTOOLCHAIN="$toolchain" CGO_ENABLED=0 go -C "$source_root" "$@"
