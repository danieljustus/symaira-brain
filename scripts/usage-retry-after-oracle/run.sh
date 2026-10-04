#!/usr/bin/env bash
# Pinned SDK parser and real owned TLS constructor proof; no provider endpoints.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"
source "$repo_root/scripts/run-external-env.sh"
external_env
python3 "$repo_root/scripts/usage-retry-after-oracle/oracle.py" "${1:?usage: run.sh OUTPUT_JSON}"
