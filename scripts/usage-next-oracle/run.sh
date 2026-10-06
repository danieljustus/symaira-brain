#!/usr/bin/env bash
# Complete early-argv/Kimi-device/real-status proof, synthetic owned peers only.
set -euo pipefail
umask 022
repo_root=$(git rev-parse --show-toplevel)
cd "$repo_root"
source "$repo_root/scripts/run-external-env.sh"
external_env
output=${1:?usage: run.sh OUTPUT_JSON}
# Native Python owns temp paths; its platform spelling avoids Bash/tar ambiguity.
python3 "$repo_root/scripts/usage-next-oracle/device.py" "$output"
