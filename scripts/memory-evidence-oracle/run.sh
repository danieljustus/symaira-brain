#!/usr/bin/env bash
# Supplemental oracle: existing Go production source and fixtures stay frozen.
set -euo pipefail
umask 022
repo=$(git rev-parse --show-toplevel)
python3 "$repo/scripts/memory-evidence-oracle/replay.py" "$@"
