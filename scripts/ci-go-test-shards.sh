#!/usr/bin/env bash
# Split and merge the sharded Go race suite used by the `build-test` CI job.
#
# The Go race suite serializes package processes (`-p 1`) because several
# packages use default XDG paths and would otherwise share one fixture
# database. Serializing is a correctness requirement, not a speed choice, so
# the wall clock is cut by running several *shards* on separate runners
# instead of relaxing that flag. Every package still runs exactly once with
# the identical flags; only the runner changes.
#
# Shard assignment is a deterministic greedy bin packing over the measured
# package durations in `scripts/ci-go-test-weights.tsv`. Unknown packages fall
# back to the median weight, so a stale table degrades balance, never coverage.
#
# Usage:
#   scripts/ci-go-test-shards.sh --verify <count>
#   scripts/ci-go-test-shards.sh --run <index> <count> <out-dir>
#   scripts/ci-go-test-shards.sh --merge <count> <in-dir> <out-dir>
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
weights_file="$repo_root/scripts/ci-go-test-weights.tsv"

usage() {
  echo "usage: $0 --verify <count> | --run <index> <count> <out-dir> | --merge <count> <in-dir> <out-dir>" >&2
  exit 2
}

weighted_packages() {
  go list ./... | sort > "$tmp_packages"
  awk -F'\t' '!/^#/ && NF >= 2 { w[$1] = $2 } END {
    while ((getline pkg < pfile) > 0) {
      if (pkg in w) print pkg "\t" w[pkg]; else print pkg "\t" unknown_weight;
    }
  }' pfile="$tmp_packages" unknown_weight="$unknown" "$weights_file" | sort -k2,2nr -k1,1
}

assign() {
  # greedy: heaviest package first, always into the currently lightest shard
  weighted_packages | awk -F'\t' -v count="$1" '{
    best = 0
    for (i = 1; i < count; i++) if (sum[i] < sum[best]) best = i
    sum[best] += $2
    print best "\t" $1 "\t" $2
  }'
}

print_report() {
  assignment=$(assign "$1")
  printf '%s\n' "$assignment" | awk -F'\t' '{ count[$1] += 1; seconds[$1] += $3 } END {
    for (i = 0; i < shards; i++) printf "shard %s: %d packages, %d s\n", i, count[i] + 0, seconds[i] + 0
  }' shards="$1"
}

verify() {
  assignment=$(assign "$1")
  packages=$(printf '%s\n' "$assignment" | awk -F'\t' '{ print $2 }' | sort)
  unique=$(printf '%s\n' "$packages" | sort -u)
  if [ "$packages" != "$unique" ]; then
    echo "shard assignment repeats a package" >&2
    exit 1
  fi
  if [ "$(printf '%s\n' "$packages" | wc -l)" != "$(wc -l < "$tmp_packages")" ]; then
    echo "shard assignment lost a package" >&2
    exit 1
  fi
  print_report "$1"
  echo "verified: every one of $(wc -l < "$tmp_packages") packages is assigned exactly once"
}

run_shard() {
  index=$1
  count=$2
  out_dir=$3
  mkdir -p "$out_dir"
  profile="$out_dir/coverage-$index.out"
  log="$out_dir/test-$index.log"
  # `index` is an awk built-in function name, so the shard number travels as `want`.
  selected=$(assign "$count" | awk -F'\t' -v want="$index" '$1 == want { print $2 }')
  if [ -z "$selected" ]; then
    # An empty shard still has to leave a parseable (header-only) profile.
    printf 'mode: set\n' > "$profile"
    : > "$log"
    echo "shard $index has no packages"
    return 0
  fi
  # shellcheck disable=SC2086  # the package list is a deliberate word split
  go test -race -shuffle=on -short -p 1 -coverprofile="$profile" $selected 2>&1 | tee "$log"
  echo "shard $index ran $(printf '%s\n' "$selected" | wc -l) packages"
}

merge_profiles() {
  count=$1
  in_dir=$2
  out_dir=$3
  mkdir -p "$out_dir"
  profile="$out_dir/coverage.out"
  log="$out_dir/test.log"
  : > "$profile"
  : > "$log"
  header_written=false
  index=0
  while [ "$index" -lt "$count" ]; do
    fragment="$in_dir/coverage-$index.out"
    if [ -f "$fragment" ]; then
      if [ "$header_written" = false ]; then
        head -n 1 "$fragment" >> "$profile"
        header_written=true
      fi
      tail -n +2 "$fragment" >> "$profile"
    fi
    if [ -f "$in_dir/test-$index.log" ]; then
      cat "$in_dir/test-$index.log" >> "$log"
    fi
    index=$((index + 1))
  done
  if [ "$header_written" = false ]; then
    printf 'mode: set\n' > "$profile"
  fi
  echo "merged $count shards into $profile and $log"
}

tmp_packages=$(mktemp "${TMPDIR:-/tmp}/ci-shards.XXXXXX")
trap 'rm -f "$tmp_packages"' EXIT INT TERM

# median (or 1 when the table is empty) weight for packages without a measurement
unknown=$(awk -F'\t' '!/^#/ && NF >= 2 { v[++n] = $2 } END {
  if (n == 0) { print 1; exit }
  for (i = 1; i <= n; i++) for (j = i + 1; j <= n; j++) if (v[j] < v[i]) { t = v[i]; v[i] = v[j]; v[j] = t }
  print v[int((n + 1) / 2)]
}' "$weights_file")

case "${1:-}" in
  --verify)
    [ $# -eq 2 ] || usage
    cd "$repo_root"
    verify "$2"
    ;;
  --run)
    [ $# -eq 4 ] || usage
    cd "$repo_root"
    run_shard "$2" "$3" "$4"
    ;;
  --merge)
    [ $# -eq 4 ] || usage
    merge_profiles "$2" "$3" "$4"
    ;;
  *) usage ;;
esac
