#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: smoke-hybrid-fallback.sh <extracted-candidate-archive>" >&2
  exit 2
fi

candidate_dir="$(cd "$1" && pwd)"
for binary in symbrain symbrain-go; do
  if [[ ! -f "$candidate_dir/$binary" || ! -x "$candidate_dir/$binary" ]]; then
    echo "hybrid fallback smoke: missing executable $binary" >&2
    exit 1
  fi
done

scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/home" "$scratch/config" "$scratch/data" "$scratch/cache" "$scratch/work"
unset SYMBRAIN_GO_BINARY
cd "$scratch/work"

rust_status=0
env -i PATH="$candidate_dir:/usr/bin:/bin" HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data" XDG_CACHE_HOME="$scratch/cache" "$candidate_dir/symbrain" skills targets --candidate-smoke-unknown >"$scratch/rust.stdout" 2>"$scratch/rust.stderr" || rust_status=$?

go_status=0
env -i PATH="$candidate_dir:/usr/bin:/bin" HOME="$scratch/home" XDG_CONFIG_HOME="$scratch/config" XDG_DATA_HOME="$scratch/data" XDG_CACHE_HOME="$scratch/cache" "$candidate_dir/symbrain-go" skills targets --candidate-smoke-unknown >"$scratch/go.stdout" 2>"$scratch/go.stderr" || go_status=$?

if [[ "$rust_status" -eq 0 || "$rust_status" -ne "$go_status" ]]; then
  echo "hybrid fallback smoke: exit codes differ (Rust=$rust_status Go=$go_status)" >&2
  exit 1
fi
if ! cmp -s "$scratch/rust.stdout" "$scratch/go.stdout" || ! cmp -s "$scratch/rust.stderr" "$scratch/go.stderr"; then
  echo "hybrid fallback smoke: Rust fallback output differs from the Go oracle" >&2
  diff -u "$scratch/go.stdout" "$scratch/rust.stdout" >&2 || true
  diff -u "$scratch/go.stderr" "$scratch/rust.stderr" >&2 || true
  exit 1
fi
if [[ ! -s "$scratch/go.stderr" ]]; then
  echo "hybrid fallback smoke: Go parser did not report the malformed flag" >&2
  exit 1
fi

echo "hybrid-fallback-smoke: PASS Rust reached PATH symbrain-go and matched Go exit/stdout/stderr"
