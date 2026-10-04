#!/usr/bin/env bash
# Prepare an immutable Go archive and invoke the actual historical-state gate.
set -euo pipefail
umask 022
repo=$(git rev-parse --show-toplevel)
oracle_ref=dcddcef0df5789123c7c9a7ebe6e01f10e941f2c
if [[ $# != 2 || $1 != --output ]]; then
  echo 'usage: run.sh --output NEW_REPORT_DIRECTORY' >&2
  exit 2
fi
report=$2
python3 "$repo/scripts/memory-historical-oracle/checker.py" --output "$report-checker.json"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/source"
git -C "$repo" archive "$oracle_ref" | tar -x -C "$work/source"
suffix=
if [[ ${OS:-} == Windows_NT ]]; then suffix=.exe; fi
(
  cd "$work/source"
  go build -o "$work/symbrain-go$suffix" ./cmd/symbrain
)
native_target=${CARGO_TARGET_DIR:-$repo/target}
python3 "$repo/scripts/memory-historical-oracle/replay.py" \
  --go "$work/symbrain-go$suffix" --go-source "$work/source" \
  --native "$native_target/debug/symbrain$suffix" --output "$report"
python3 "$repo/scripts/memory-historical-oracle/controls.py" \
  --go "$work/symbrain-go$suffix" --go-source "$work/source" \
  --native "$native_target/debug/symbrain$suffix" --output "$report-controls"
