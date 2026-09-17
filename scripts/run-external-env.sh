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

external_env() {
  if [[ -n ${CI:-} ]]; then
    return 0
  fi
  if [[ $(uname -s) != Darwin ]]; then
    return 0
  fi

  local base mounted runtime_root
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
  runtime_root=$(cd "$runtime_root" && pwd -P)
  if [[ "$runtime_root" != "$SYMAIRA_NVME_ROOT"/* ]]; then
    echo "SYMAIRA_EXTERNAL_RUNTIME_ROOT must resolve under $SYMAIRA_NVME_ROOT" >&2
    return 2
  fi

  export TMPDIR="$base/tmp" TMP="$base/tmp" TEMP="$base/tmp"
  export GOTMPDIR="$base/go-tmp" GOPATH="$base/gopath"
  export GOTELEMETRYDIR="$base/go-telemetry" GOCACHE="$base/go-cache"
  export GOMODCACHE="$base/go-mod-cache" CARGO_HOME="$base/cargo-home"
  export CARGO_TARGET_DIR="$base/cargo-target"
  export RUSTUP_NO_UPDATE_CHECK=1
  export PYTHONPYCACHEPREFIX="$base/python-cache"
  export SYMAIRA_EXTERNAL_RUNTIME_ROOT="$runtime_root"
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
