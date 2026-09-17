#!/bin/sh
# build-module-packages.sh — build the optional Brain module binaries
# (symbrowse, symoperate, symscope) from the in-repo receiving sources and
# package them as local replacement archives with checksums and a
# provenance manifest.
#
# This is the local, pre-publication distribution path of PB-2026-09-09:
# the archives are Ersatzkandidaten for local acceptance and migration
# testing. Nothing here is published; Homebrew tap changes stay
# unpublished until the release gates in the acceptance register clear.
#
# Usage: scripts/build-module-packages.sh [--root <repo>] [--out <dir>]
#        [--modules browse,operate,scope]
#
# Output layout (default external NVMe artifact root locally, <repo>/dist/modules/ in CI):
#   symbrowse_<version>_<os>_<arch>.tar.gz
#   symoperate_<version>_<os>_<arch>.tar.gz   (macOS only)
#   symscope_<version>_<os>_<arch>.tar.gz     (macOS only)
#   checksums.txt                             (SHA-256 over every archive)
#   provenance.json                           (origin, commit, toolchain)
#
# Repeatability: the Go module builds with -trimpath and CGO_ENABLED=0.
# SwiftPM builds are not bit-for-bit reproducible across toolchains; the
# provenance manifest records the exact toolchain string so a package is
# always attributable, and the acceptance harness verifies behavior (MCP
# handshake, catalog, lifecycle) rather than archive bytes.
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

# Local builds inherit the repository-wide NVMe cache and temp paths. CI keeps
# the existing workspace-local behavior through the same wrapper.
if [ -z "${SYMAIRA_EXTERNAL_ENV_READY:-}" ]; then
    export SYMAIRA_EXTERNAL_ENV_READY=1
    exec bash "$ROOT/scripts/run-external-env.sh" "$ROOT/scripts/build-module-packages.sh" "$@"
fi

OUT=""
MODULES="browse,operate,scope"
HOST_OS=$(uname -s | tr '[:upper:]' '[:lower:]')

while [ $# -gt 0 ]; do
    case "$1" in
        --root) ROOT=$2; shift 2 ;;
        --out) OUT=$2; shift 2 ;;
        --modules) MODULES=$2; shift 2 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

SYMAIRA_NVME_ROOT=/Volumes/1TB_NVMe_SN850X
if [ -n "${CI:-}" ] || [ "$HOST_OS" != "darwin" ]; then
    DEFAULT_OUT="$ROOT/dist/modules"
else
    DEFAULT_OUT="${SYMAIRA_EXTERNAL_BASE:-$SYMAIRA_NVME_ROOT/Dev/Symaira_Dev/builds/symaira-brain}/artifacts/modules"
fi
OUT=${OUT:-"$DEFAULT_OUT"}

# Resolve existing path components before creating anything. This prevents an
# explicit local path or a symlinked output directory from reaching cleanup.
validate_output_path() {
    candidate=$1
    case "$candidate" in
        "$SYMAIRA_NVME_ROOT"/*) ;;
        *)
            echo "--out must be under $SYMAIRA_NVME_ROOT outside CI" >&2
            exit 2
            ;;
    esac

    parent=$(dirname "$candidate")
    while [ ! -d "$parent" ]; do
        next=$(dirname "$parent")
        if [ "$next" = "$parent" ]; then
            echo "cannot resolve --out parent: $candidate" >&2
            exit 2
        fi
        parent=$next
    done
    resolved_parent=$(CDPATH= cd -- "$parent" && pwd -P)
    case "$resolved_parent" in
        "$SYMAIRA_NVME_ROOT"|"$SYMAIRA_NVME_ROOT"/*) ;;
        *)
            echo "--out resolves outside $SYMAIRA_NVME_ROOT: $candidate" >&2
            exit 2
            ;;
    esac

    if [ -e "$candidate" ]; then
        resolved=$(CDPATH= cd -- "$candidate" && pwd -P)
        case "$resolved" in
            "$SYMAIRA_NVME_ROOT"/*) ;;
            *)
                echo "--out resolves outside $SYMAIRA_NVME_ROOT: $candidate" >&2
                exit 2
                ;;
        esac
    fi
}

if [ -z "${CI:-}" ] && [ "$HOST_OS" = "darwin" ]; then
    validate_output_path "$OUT"
fi

OS=$HOST_OS
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ARCH=amd64 ;;
    arm64|aarch64) ARCH=arm64 ;;
esac

mkdir -p "$OUT/.build"
rm -f "$OUT"/checksums.txt "$OUT"/provenance.json "$OUT"/.build/prov-*.json

RECEIVER_COMMIT=$(git -C "$ROOT" rev-parse HEAD)
BUILD_START=$(date -u +%Y-%m-%dT%H:%M:%SZ)

# module_selected <name>
module_selected() {
    case ",$MODULES," in
        *",$1,"*) return 0 ;;
        *) return 1 ;;
    esac
}

# package_binary <module-dir> <binary-name> <builder-string> <built-binary-path>
package_binary() {
    dir=$1; name=$2; builder=$3; bin=$4
    version=$("$bin" version --json | python3 -c 'import json,sys; print(json.load(sys.stdin)["version"])')
    case "$version" in v*) ;; *) version="v$version" ;; esac
    base="${name}_${version#v}_${OS}_${ARCH}"
    stage=$(mktemp -d "${TMPDIR:-/tmp}/symbrain-module.XXXXXX")
    cp "$bin" "$stage/$name"
    chmod 755 "$stage/$name"
    tar -C "$stage" -czf "$OUT/$base.tar.gz" "$name"
    rm -rf "$stage"
    sha=$(shasum -a 256 "$OUT/$base.tar.gz" | awk '{print $1}')
    echo "$sha  $base.tar.gz" >> "$OUT/checksums.txt"
    python3 - "$OUT/.build/prov-$dir.json" "$dir" "$name" "$version" "$base.tar.gz" "$sha" "$RECEIVER_COMMIT" "$builder" <<'PY'
import json, sys
path, module, name, version, archive, sha, commit, builder = sys.argv[1:9]
with open(path, "w") as fh:
    json.dump({
        "module": module, "binary": name, "version": version,
        "archive": archive, "sha256": sha, "source": "brain-source",
        "receiver_commit": commit, "builder": builder,
    }, fh, indent=2)
    fh.write("\n")
PY
    echo "  packaged $base.tar.gz ($sha)"
}

if module_selected browse; then
    echo "==> building symbrowse from $ROOT/browse"
    (cd "$ROOT/browse" && CGO_ENABLED=0 go build -trimpath -o "$OUT/.build/symbrowse" ./cmd/symbrowse)
    package_binary browse symbrowse "$(go version)" "$OUT/.build/symbrowse"
fi

if [ "$OS" = "darwin" ]; then
    for mod in operate scope; do
        if module_selected "$mod"; then
            case "$mod" in
                operate) bin=symoperate ;;
                scope) bin=symscope ;;
            esac
            echo "==> building $bin from $ROOT/$mod (release)"
            swift build --package-path "$ROOT/$mod" -c release --scratch-path "$OUT/.build/$mod-scratch" --cache-path "$OUT/.build/swift-cache" >/dev/null
            BINPATH=$(swift build --package-path "$ROOT/$mod" -c release --scratch-path "$OUT/.build/$mod-scratch" --cache-path "$OUT/.build/swift-cache" --show-bin-path)
            package_binary "$mod" "$bin" "$(swift --version | head -n 1)" "$BINPATH/$bin"
        fi
    done
else
    case ",$MODULES," in
        *operate*|*scope*)
            echo "note: operate/scope are darwin-only; skipped on $OS" >&2 ;;
    esac
fi

python3 - "$OUT" "$RECEIVER_COMMIT" "$BUILD_START" "$OS" "$ARCH" <<'PY'
import glob, json, os, sys
out, commit, built_at, goos, arch = sys.argv[1:6]
packages = []
for path in sorted(glob.glob(os.path.join(out, ".build", "prov-*.json"))):
    with open(path) as fh:
        packages.append(json.load(fh))
manifest = {
    "schema_version": 1,
    "generated_at": built_at,
    "receiver": "github.com/danieljustus/symaira-brain",
    "receiver_commit": commit,
    "os": goos,
    "arch": arch,
    "packages": packages,
}
with open(os.path.join(out, "provenance.json"), "w") as fh:
    json.dump(manifest, fh, indent=2)
    fh.write("\n")
PY

rm -rf "$OUT/.build"
echo "==> packages written to $OUT"
ls -la "$OUT"
