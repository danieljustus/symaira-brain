#!/usr/bin/env bash
# Local diagnostic collection only, not FETCH-002 parity acceptance.
# Require an absolute installed Go binary plus verified external cache paths:
# FETCH_GO_BINARY, FETCH_GO_CACHE, FETCH_GO_MODCACHE.
# Collection and independently approved digest verification remain separate.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
BROWSE_DIR="$REPO_ROOT/browse"
: "${FETCH_GO_BINARY:?set an absolute installed Go binary path}"
: "${FETCH_GO_CACHE:?set a verified external Go build-cache path}"
: "${FETCH_GO_MODCACHE:?set a verified external Go module-cache path}"
dev-external --status
PREFLIGHT_HOME="$HOME"
umask 077
mkdir -p "$BROWSE_DIR/target"
mkdir -p "$BROWSE_DIR/target/fetch002-wire-next"
EXEC_DIR="$(mktemp -d "$BROWSE_DIR/target/fetch002-wire-next/capture.XXXXXXXX")"
export HOME="$EXEC_DIR/home" XDG_CONFIG_HOME="$EXEC_DIR/config"
export XDG_DATA_HOME="$EXEC_DIR/data" XDG_CACHE_HOME="$EXEC_DIR/cache"
export XDG_STATE_HOME="$EXEC_DIR/state" XDG_RUNTIME_DIR="$EXEC_DIR/run"
export TMPDIR="$EXEC_DIR/tmp" GOTMPDIR="$EXEC_DIR/gotmp" GOPATH="$EXEC_DIR/gopath"
export GOCACHE="$FETCH_GO_CACHE" GOMODCACHE="$FETCH_GO_MODCACHE"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$XDG_DATA_HOME" "$XDG_CACHE_HOME" \
  "$XDG_STATE_HOME" "$XDG_RUNTIME_DIR" "$TMPDIR" "$GOTMPDIR" "$GOPATH"
export CGO_ENABLED=0 GOTOOLCHAIN=local GOMAXPROCS=2 GOWORK=off GOPROXY=off
export GOFLAGS="-mod=readonly"
export PATH="$(dirname "$FETCH_GO_BINARY"):$PATH"
cd "$BROWSE_DIR"
"$FETCH_GO_BINARY" list -deps -test -json ./internal/fetch/fetch > "$EXEC_DIR/go-list.json"
snapshot_inputs() {
  python3 - "$EXEC_DIR/go-list.json" <<'PY'
import hashlib, json, pathlib, sys
text = pathlib.Path(sys.argv[1]).read_text()
decoder = json.JSONDecoder()
files = set()
while text.strip():
    text = text.lstrip()
    obj, end = decoder.raw_decode(text)
    text = text[end:]
    if not obj.get('Dir'):
        continue
    for key in ('GoFiles', 'CgoFiles', 'SFiles', 'SysoFiles', 'CFiles', 'HFiles',
                'CXXFiles', 'MFiles', 'FFiles', 'EmbedFiles'):
        for name in obj.get(key, []):
            files.add(pathlib.Path(obj['Dir'], name).resolve())
files.update(pathlib.Path(name).resolve() for name in ('go.mod', 'go.sum'))
print(json.dumps({str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in sorted(files)}, indent=2, sort_keys=True))
PY
}
snapshot_inputs > "$EXEC_DIR/inputs-before.json"
HOME="$PREFLIGHT_HOME" dev-external --status > "$EXEC_DIR/preflight-build.log"
TEST_BIN="$EXEC_DIR/fetch_fingerprints_capture_test"
"$FETCH_GO_BINARY" test -p=2 -c -o "$TEST_BIN" ./internal/fetch/fetch/ 2>&1 | tee "$EXEC_DIR/build.log"
shasum -a 256 "$TEST_BIN" > "$EXEC_DIR/binary-sha256.txt"
"$FETCH_GO_BINARY" version -m "$TEST_BIN" > "$EXEC_DIR/binary-build-info.log"
CAPTURE_OUT="$EXEC_DIR/capture.json"
(cd "$BROWSE_DIR/internal/fetch/fetch" && \
  SYMBROWSE_FETCH_FINGERPRINTS_OUT="$CAPTURE_OUT" "$TEST_BIN" \
  -test.run '^TestGenerateFetchFingerprintsCapture$' -test.v -test.timeout=90s) \
  2>&1 | tee "$EXEC_DIR/capture.log"
snapshot_inputs > "$EXEC_DIR/inputs-after.json"
cmp "$EXEC_DIR/inputs-before.json" "$EXEC_DIR/inputs-after.json"
echo "COLLECTED: $CAPTURE_OUT — independently review before accepting."
echo "Validate separately with --trusted-sha256 and --go; no self-approval or parity claim."
