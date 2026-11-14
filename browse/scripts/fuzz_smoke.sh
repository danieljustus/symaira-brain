#!/usr/bin/env bash
# Bounded libFuzzer smoke over a scratch copy of every tracked seed corpus.
#
# libFuzzer writes new interesting inputs into the corpus directory it is given,
# so tracked seeds are never passed directly: each target runs on a temporary
# copy, and the tracked digests are verified before and after. Run from
# anywhere; wrap in `dev-external` so build output stays outside the repository.
#   FUZZ_RUNS      executions per target (default 64, 1..4096)
#   FUZZ_MAX_LEN   maximum input length in bytes (default 65536)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

runs="${FUZZ_RUNS:-64}"
max_len="${FUZZ_MAX_LEN:-65536}"
case "$runs" in ''|*[!0-9]*) echo "FUZZ_RUNS must be an integer" >&2; exit 2;; esac
if [ "$runs" -lt 1 ] || [ "$runs" -gt 4096 ]; then
  echo "FUZZ_RUNS must be between 1 and 4096" >&2
  exit 2
fi

# <cargo-fuzz target> <corpus directory>
targets=(
  "daemon-mcp-frames daemon_mcp_frames"
  "dom-input dom_input"
  "fetch-text fetch_text"
  "flow-yaml flow_yaml"
  "http-body-decode http_body_decode"
  "state-headers state_headers"
  "urls urls"
)

scratch="$(mktemp -d "${TMPDIR:-/tmp}/symbrowse-fuzz.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT INT TERM

python3 scripts/fuzz_corpus_hashes.py --check
cargo +nightly fuzz build

for entry in "${targets[@]}"; do
  read -r target corpus <<<"$entry"
  work="$scratch/$corpus"
  mkdir -p "$work"
  cp "fuzz/corpus/$corpus"/* "$work/"
  log="$scratch/$target.log"
  cargo +nightly fuzz run "$target" "$work" -- \
    "-runs=$runs" "-max_len=$max_len" -rss_limit_mb=2048 >"$log" 2>&1 || {
    tail -n 40 "$log" >&2
    echo "fuzz smoke FAILED: $target" >&2
    exit 1
  }
  done_line="$(grep -E '^Done [0-9]+ runs' "$log" | tail -n 1 || true)"
  if [ -z "$done_line" ]; then
    tail -n 20 "$log" >&2
    echo "fuzz smoke: $target did not report a completed bounded run" >&2
    exit 1
  fi
  echo "$target: $done_line"
done

python3 scripts/fuzz_corpus_hashes.py --check
