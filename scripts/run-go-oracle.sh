#!/usr/bin/env bash
# Run a Go oracle command from an immutable Git revision in an isolated export.
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "usage: $0 <git-revision> <go-arguments...>" >&2
  exit 2
fi

revision=$1
shift
repo_root=$(git rev-parse --show-toplevel)
commit=$(git -C "$repo_root" rev-parse --verify "${revision}^{commit}")
source_root=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-go-oracle.XXXXXX")
trap 'rm -rf "$source_root"' EXIT INT TERM

git -C "$repo_root" archive --format=tar "$commit" | tar -xf - -C "$source_root"
toolchain=$(awk '$1 == "go" { print "go" $2; exit }' "$source_root/go.mod")
if [[ -z "$toolchain" ]]; then
  echo "cannot determine Go toolchain from oracle revision $commit" >&2
  exit 1
fi

GOTOOLCHAIN="$toolchain" CGO_ENABLED=0 go -C "$source_root" "$@"
