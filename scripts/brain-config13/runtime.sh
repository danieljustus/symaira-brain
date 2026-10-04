#!/usr/bin/env bash
# One allocated Linux target; retain every strict gate and failed observation.
set -euo pipefail
umask 022
root=$(git rev-parse --show-toplevel)
out=${1:?usage: runtime.sh NEW_ABSOLUTE_OUTPUT_DIRECTORY}
test "${BRAIN765_RUNTIME_ALLOCATION:-}" = "$root/target"
test "${BRAIN765_PORT11434_ALLOCATION:-}" = "$root"
test -d "$root/target"
test ! -e "$out"
mkdir -p "$out"
out=$(realpath "$out")
. /home/agent/.cargo/env
export CARGO_TARGET_DIR="$root/target" CARGO_HOME=/home/agent/.cargo RUSTUP_HOME=/home/agent/.rustup
export CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=2
export CARGO_NET_OFFLINE=true GOPROXY=off GOSUMDB=off GOTOOLCHAIN=go1.26.7 GOWORK=off GOENV=off CGO_ENABLED=0
export PATH=/workspace/toolchains/go1.26.7/bin:$PATH PYTHONDONTWRITEBYTECODE=1 SYMBRAIN_VERSION=dev
mkdir -p "$out/private-home" "$out/go-cache" "$out/tmp"
export HOME="$out/private-home" GOPATH="$out/go-path" GOMODCACHE=/home/agent/go/pkg/mod GOCACHE="$out/go-cache"
export TMPDIR="$out/tmp" TMP="$out/tmp" TEMP="$out/tmp"
unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_CACHE_HOME XDG_STATE_HOME
cd "$root"
python3 scripts/brain-config13/runtime_tools.py source --root "$root" --out "$out"
head=$(git rev-parse HEAD)
failures=0
floor() { python3 - "$root" <<'PY'
import shutil,sys
free=shutil.disk_usage(sys.argv[1]).free
assert free >=700*1024*1024, f"stop below700MiB: {free}"
PY
}
stage() {
  local name=$1; shift
  floor || exit 70
  test ! -e "$out/$name.stdout"
  printf '%s\n' "$name" >> "$out/stages.started"
  if "$@" > "$out/$name.stdout" 2> "$out/$name.stderr"; then
    printf '%s 0\n' "$name" >> "$out/stages.exit"
  else
    local code=$?
    printf '%s %s\n' "$name" "$code" >> "$out/stages.exit"
    failures=$((failures+1))
    return "$code"
  fi
}
if ! stage build cargo build --workspace --all-targets --all-features --locked --message-format=json-render-diagnostics; then
  python3 scripts/brain-config13/runtime_tools.py archive --root "$root" --out "$out" --target "$CARGO_TARGET_DIR"
  exit 1
fi
python3 scripts/brain-config13/runtime_tools.py binaries --root "$root" --out "$out" --target "$CARGO_TARGET_DIR"
stage tests cargo test --workspace --all-targets --all-features --locked || true
stage clippy cargo clippy --workspace --all-targets --all-features --locked -- -D warnings || true
stage fmt cargo fmt --all -- --check || true
stage actionlint /workspace/toolchains/bin/actionlint || true
mkdir "$out/frozen-go"
git archive dcddcef0df5789123c7c9a7ebe6e01f10e941f2c | tar -xf - -C "$out/frozen-go"
stage go-cli go -C "$out/frozen-go" build -mod=readonly -ldflags '-X main.version=dev' -o "$out/go-cli" ./cmd/symbrain
stage stage-go python3 scripts/brain-config13/stage_go.py --frozen /workspace/oracles/daemon772-go-source --owned-module "$out/go-probe-source"
stage go-probe go -C "$out/go-probe-source" build -mod=readonly -o "$out/go-probe" ./cmd/config13-probe
common=(--source-root "$root" --source-head "$head")
stage cli102 python3 scripts/brain-config13/process.py --go "$out/go-cli" --native "$out/bin/cli" "${common[@]}" --out "$out/cli102.json" || true
stage values152 python3 scripts/brain-config13/values.py --go "$out/go-probe" --native "$out/bin/brain_config13_probe" "${common[@]}" --out "$out/values152.json" || true
correction=("${common[@]}" --go-contract-root "$out/frozen-go")
stage consumers182 python3 scripts/brain-config13/corrections.py --kind consumers --go "$out/go-cli" --native "$out/bin/cli" --vault-fixture "$out/bin/brain_config13_owned_vault" "${correction[@]}" --out "$out/consumers182.json" || true
stage corrections146 python3 scripts/brain-config13/corrections.py --kind values --go "$out/go-probe" --native "$out/bin/brain_config13_probe" "${correction[@]}" --out "$out/corrections146.json" || true
for pair in 'wrong-owner vault-pwd-different-False' 'repair-codex profile-incomplete-codex-True' 'reject-prefix prefix-fffe-global-codex-False'; do
  read -r mode case_id <<< "$pair"
  if stage "control-$mode" python3 scripts/brain-config13/corrections.py --kind consumers --go "$out/go-cli" --native "$out/bin/brain_config13_correction_fault" --control-native "$out/bin/cli" --control-mode "$mode" --case "$case_id" --vault-fixture "$out/bin/brain_config13_owned_vault" "${correction[@]}" --out "$out/control-$mode.json"; then
    echo "incorrectly accepted control $mode" >> "$out/control-errors"
    failures=$((failures+1))
  else
    # An intended rejection is verified separately from incidental wrapper failure.
    if stage "control-verified-$mode" python3 scripts/brain-config13/runtime_tools.py control --root "$root" --out "$out" --report "$out/control-$mode.json" --family correction --control-mode "$mode"; then
      failures=$((failures-1))
    fi
  fi
done
for family in cli values; do
  modes='premature-fs wrong-exit wrong-field-error'
  if [ "$family" = cli ]; then modes="$modes premature-http"; else modes="$modes wrong-value"; fi
  for mode in $modes; do
    if [ "$family" = cli ]; then
      command=(python3 scripts/brain-config13/process.py --go "$out/go-cli" --case audit.enabled-install --control-native "$out/bin/cli")
    else
      case_id=wrong-global-audit.enabled
      if [ "$mode" = wrong-value ]; then case_id=defaults; fi
      command=(python3 scripts/brain-config13/values.py --go "$out/go-probe" --case "$case_id" --control-native "$out/bin/brain_config13_probe")
    fi
    report="$out/control-$family-$mode.json"
    if stage "control-$family-$mode" "${command[@]}" --native "$out/bin/brain_config13_fault" --control-mode "$mode" "${common[@]}" --out "$report"; then
      echo "incorrectly accepted control $family $mode" >> "$out/control-errors"
      failures=$((failures+1))
    elif stage "control-verified-$family-$mode" python3 scripts/brain-config13/runtime_tools.py control --root "$root" --out "$out" --report "$report" --family "$family" --control-mode "$mode"; then
      failures=$((failures-1))
    fi
  done
done
python3 scripts/brain-config13/runtime_tools.py historical --root "$root" --out "$out"
for name in inventory boundaries patterns admission; do
  stage "historical-$name" python3 "$out/historical/symaira-brain-config13-$name.py" || true
done
for helper in "$out"/historical/symaira-doctor765-sixth-independent-supplement-*.py; do
  stage "$(basename "$helper" .py)" python3 "$helper" || true
done
for family in setup-source doctor-repair setup-repair guard-standalone; do
  stage "$family" bash "scripts/$family-oracle/run.sh" "$out/$family.json" || true
done
for family in setup-output doctor-stdout; do
  stage "$family" bash "scripts/$family-oracle/run.sh" "$out/$family" || true
done
stage memory590 python3 scripts/memory-cli-oracle/replay.py --rust "$out/bin/cli" --report "$out/memory590.json" || true
stage memory-controls python3 scripts/memory-cli-oracle/controls.py --rust "$out/bin/cli" --go "$out/go-cli" --report "$out/memory-controls.json" || true
stage memory-writes python3 scripts/memory-cli-oracle/write_gate.py --go "$out/go-cli" --rust "$out/bin/cli" --report-dir "$out/memory-writes" || true
stage doctests cargo test --workspace --all-features --locked --doc || true
printf '%s\n' "$failures" > "$out/unclassified_nonzero_stages"
# Raw stage exits/report content remain the authority; no aggregate PASS projection.
if [ "$failures" -ne 0 ]; then
  python3 scripts/brain-config13/runtime_tools.py archive --root "$root" --out "$out" --target "$CARGO_TARGET_DIR"
fi
test "$failures" -eq 0
