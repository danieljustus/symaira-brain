#!/usr/bin/env bash
# Keep local tool caches, temporary files, and generated artifacts on the NVMe.
set -euo pipefail

SYMAIRA_NVME_ROOT=/Volumes/1TB_NVMe_SN850X

validate_external_path() {
  local name=$1 candidate=$2 parent next resolved
  case "$candidate" in
    "$SYMAIRA_NVME_ROOT"/*) ;;
    *)
      echo "$name must resolve under $SYMAIRA_NVME_ROOT" >&2
      return 2
      ;;
  esac

  parent=$candidate
  while [[ ! -e $parent ]]; do
    next=$(dirname "$parent")
    if [[ $next == "$parent" ]]; then
      echo "cannot resolve $name: $candidate" >&2
      return 2
    fi
    parent=$next
  done
  if [[ ! -d $parent ]]; then
    echo "$name parent is not a directory: $parent" >&2
    return 2
  fi
  resolved=$(cd "$parent" && pwd -P)
  if [[ $resolved != "$SYMAIRA_NVME_ROOT" && $resolved != "$SYMAIRA_NVME_ROOT"/* ]]; then
    echo "$name must resolve under $SYMAIRA_NVME_ROOT" >&2
    return 2
  fi
}

external_env_ready() {
  if [[ -n ${CI:-} ]] || [[ $(uname -s) != Darwin ]]; then
    return 0
  fi
  if [[ ${SYMAIRA_EXTERNAL_ENV_READY:-} != 1 ]]; then
    return 1
  fi

  local mounted name candidate
  mounted=$(df -P "$SYMAIRA_NVME_ROOT" 2>/dev/null | awk 'NR == 2 {print $NF}')
  if [[ $mounted != "$SYMAIRA_NVME_ROOT" ]]; then
    return 1
  fi
  for name in TMPDIR TMP TEMP GOTMPDIR GOPATH GOTELEMETRYDIR GOCACHE GOMODCACHE \
    CARGO_HOME CARGO_TARGET_DIR PYTHONPYCACHEPREFIX SYMAIRA_EXTERNAL_RUNTIME_ROOT \
    GUARD_DECIDE_EVIDENCE GUARD_REPAIR_OUTPUT; do
    candidate=${!name:-}
    if [[ -z $candidate ]] || ! validate_external_path "$name" "$candidate" >/dev/null 2>&1; then
      return 1
    fi
  done
}

external_env() {
  if [[ -n ${CI:-} ]]; then
    local ci_tmp=${RUNNER_TEMP:-/tmp}
    [[ -d $ci_tmp ]] || ci_tmp=/tmp
    export TMPDIR="$ci_tmp" TMP="$ci_tmp" TEMP="$ci_tmp"
    return 0
  fi
  if [[ $(uname -s) != Darwin ]]; then
    return 0
  fi

  local base mounted runtime_root evidence repair_output worktree_root worktree_key
  base=${SYMAIRA_EXTERNAL_BASE:-$SYMAIRA_NVME_ROOT/Dev/Symaira_Dev/builds/symaira-brain}
  runtime_root=${SYMAIRA_EXTERNAL_RUNTIME_ROOT:-$SYMAIRA_NVME_ROOT/tmp}
  mounted=$(df -P "$SYMAIRA_NVME_ROOT" 2>/dev/null | awk 'NR == 2 {print $NF}')
  if [[ "$mounted" != "$SYMAIRA_NVME_ROOT" ]]; then
    echo "external build storage is not mounted at $SYMAIRA_NVME_ROOT" >&2
    return 2
  fi

  validate_external_path "SYMAIRA_EXTERNAL_BASE" "$base"
  validate_external_path "SYMAIRA_EXTERNAL_RUNTIME_ROOT" "$runtime_root"

  mkdir -p "$base" "$runtime_root"
  base=$(cd "$base" && pwd -P)
  if [[ "$base" != "$SYMAIRA_NVME_ROOT"/* ]]; then
    echo "SYMAIRA_EXTERNAL_BASE must resolve under $SYMAIRA_NVME_ROOT" >&2
    return 2
  fi
  worktree_root=$(git rev-parse --show-toplevel 2>/dev/null || pwd -P)
  worktree_key=$(printf '%s' "$worktree_root" | shasum -a 256 | cut -c1-16)
  runtime_root=$(cd "$runtime_root" && pwd -P)
  if [[ "$runtime_root" != "$SYMAIRA_NVME_ROOT"/* ]]; then
    echo "SYMAIRA_EXTERNAL_RUNTIME_ROOT must resolve under $SYMAIRA_NVME_ROOT" >&2
    return 2
  fi

  evidence=${GUARD_DECIDE_EVIDENCE:-$base/guard-decide-provenancefix}
  repair_output=${GUARD_REPAIR_OUTPUT:-$base/guard-decide-raw-byte}
  validate_external_path "GUARD_DECIDE_EVIDENCE" "$evidence"
  validate_external_path "GUARD_REPAIR_OUTPUT" "$repair_output"

  export TMPDIR="$base/tmp" TMP="$base/tmp" TEMP="$base/tmp"
  export GOTMPDIR="$base/go-tmp" GOPATH="$base/gopath"
  export GOTELEMETRYDIR="$base/go-telemetry" GOCACHE="$base/go-cache"
  export GOMODCACHE="$base/go-mod-cache" CARGO_HOME="$base/cargo-home"
  # Cargo test binaries must not be shared across linked worktrees: the same
  # package/version can otherwise resolve to another checkout's executable.
  export CARGO_TARGET_DIR="$base/cargo-target/$worktree_key"
  export RUSTUP_NO_UPDATE_CHECK=1
  export PYTHONPYCACHEPREFIX="$base/python-cache"
  export SYMAIRA_EXTERNAL_RUNTIME_ROOT="$runtime_root"
  export GUARD_DECIDE_EVIDENCE="$evidence"
  export GUARD_REPAIR_OUTPUT="$repair_output"
  export SYMAIRA_EXTERNAL_ENV_READY=1
  mkdir -p "$TMPDIR" "$GOTMPDIR" "$GOPATH" "$GOTELEMETRYDIR" "$GOCACHE" \
    "$GOMODCACHE" "$CARGO_HOME" "$CARGO_TARGET_DIR" "$PYTHONPYCACHEPREFIX" \
    "$SYMAIRA_EXTERNAL_RUNTIME_ROOT"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  if [[ $# -eq 0 ]]; then
    echo "usage: $0 <command> [argument ...]" >&2
    exit 2
  fi
  external_env
  exec "$@"
fi
