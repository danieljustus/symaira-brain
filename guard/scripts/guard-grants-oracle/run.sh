#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
action=check
if [[ $# -gt 0 ]]; then
  action=$1
fi
case "$action" in
  write|check)
    exec bash "$repo_root/scripts/run-external-env.sh" python3 "$repo_root/guard/scripts/guard-grants-oracle/oracle.py" "$action"
    ;;
  test)
    exec bash "$repo_root/scripts/run-external-env.sh" python3 -m unittest discover -s "$repo_root/guard/scripts/guard-grants-oracle" -p 'test_*.py'
    ;;
  parity)
    if [[ $# -ne 2 ]]; then
      echo "usage: $0 parity /path/to/symbrain" >&2
      exit 2
    fi
    exec bash "$repo_root/scripts/run-external-env.sh" python3 "$repo_root/guard/scripts/guard-grants-oracle/parity.py" "$2"
    ;;
  *)
    echo "usage: $0 {write|check|test|parity /path/to/symbrain}" >&2
    exit 2
    ;;
esac
