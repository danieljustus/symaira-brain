#!/usr/bin/env bash
# Build, export and upload the Symaira Brain iOS companion to TestFlight.
#
# `preflight` needs no Apple account. `archive`, `export` and `upload` take an
# App Store Connect API key from the environment; SymVault injects it without
# exposing the values:
#
#   scripts/ios-testflight.sh preflight
#   symvault run --env ASC_API_KEY=apple-developer/notary-api-key.password \
#     --env ASC_KEY_ID=apple-developer/notary-api-key-id.password \
#     --env ASC_ISSUER_ID=apple-developer/notary-api-issuer-id.password -- \
#     scripts/ios-testflight.sh upload
#
# BUILD_NUMBER defaults to a UTC timestamp so local and CI uploads stay
# monotonic. Build output lives under IOS_BUILD_ROOT, outside the repository.
# `symvault run` clears the environment: pass these after `--` via `env`.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROJECT="$REPO_ROOT/SymBrain.xcodeproj"
SCHEME="SymBrainMobile"
APP_NAME="Symaira Brain.app"
BUNDLE_ID="com.symaira.brain.mobile"
TEAM_ID="${TEAM_ID:-M4744F3TAA}"
BUILD_NUMBER="${BUILD_NUMBER:-$(date -u +%Y%m%d%H%M)}"
IOS_BUILD_ROOT="${IOS_BUILD_ROOT:-${TMPDIR:-/tmp}/symbrain-ios-testflight}"
ARCHIVE_PATH="$IOS_BUILD_ROOT/SymBrainMobile.xcarchive"
DERIVED_DATA="$IOS_BUILD_ROOT/derived-data"
EXPORT_PATH="$IOS_BUILD_ROOT/export"
EXPORT_OPTIONS="$REPO_ROOT/scripts/ios/ExportOptions-TestFlight.plist"
UPLOAD_OPTIONS="$REPO_ROOT/scripts/ios/ExportOptions-TestFlight-Upload.plist"
AUTH_ARGS=()
AUTH_KEY_FILE=""

if [[ -z "${DEVELOPER_DIR:-}" ]]; then
  for candidate in /Applications/Xcode-*.app /Applications/Xcode.app; do
    [[ -d "$candidate/Contents/Developer" ]] || continue
    [[ "$candidate" == *-beta.app ]] && continue
    export DEVELOPER_DIR="$candidate/Contents/Developer"
    break
  done
fi

cleanup_auth() {
  [[ -z "$AUTH_KEY_FILE" ]] || rm -f "$AUTH_KEY_FILE"
}
trap cleanup_auth EXIT

die() {
  printf 'error: %s\n' "$1" >&2
  exit 1
}

prepare_auth() {
  AUTH_ARGS=(-allowProvisioningUpdates)
  [[ -n "${ASC_API_KEY:-}" ]] || die "ASC_API_KEY is required; inject it from SymVault"
  : "${ASC_KEY_ID:?ASC_KEY_ID is required with ASC_API_KEY}"
  : "${ASC_ISSUER_ID:?ASC_ISSUER_ID is required with ASC_API_KEY}"
  AUTH_KEY_FILE="$(mktemp "${TMPDIR:-/tmp}/symbrain-asc-key.XXXXXX")"
  chmod 600 "$AUTH_KEY_FILE"
  # The key may be stored raw (PEM) or base64-encoded (CI secret).
  if printf '%s' "$ASC_API_KEY" | base64 --decode >"$AUTH_KEY_FILE" 2>/dev/null &&
    /usr/bin/grep -q -- 'BEGIN PRIVATE KEY' "$AUTH_KEY_FILE"; then
    :
  else
    printf '%s' "$ASC_API_KEY" >"$AUTH_KEY_FILE"
  fi
  AUTH_ARGS+=(
    -authenticationKeyPath "$AUTH_KEY_FILE"
    -authenticationKeyID "$ASC_KEY_ID"
    -authenticationKeyIssuerID "$ASC_ISSUER_ID"
  )
}

require_tools() {
  command -v xcodegen >/dev/null 2>&1 || die "xcodegen is required"
  [[ -x "${DEVELOPER_DIR:-}/usr/bin/xcodebuild" ]] || die "full Xcode not found"
  [[ -d "$DEVELOPER_DIR/Platforms/iPhoneOS.platform" ]] || die "iPhoneOS platform missing from the selected Xcode"
}

prepare_project() {
  (cd "$REPO_ROOT" && xcodegen generate --quiet)
  mkdir -p "$IOS_BUILD_ROOT"
}

check_app() {
  local app="$1"
  [[ -d "$app" ]] || die "expected iPhone app not found at $app"
  local found
  found=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Info.plist")
  [[ "$found" == "$BUNDLE_ID" ]] || die "unexpected bundle ID: $found"
}

preflight() {
  require_tools
  prepare_project
  rm -rf "$DERIVED_DATA"
  xcodebuild -project "$PROJECT" -scheme "$SCHEME" -configuration Release \
    -sdk iphoneos -destination 'generic/platform=iOS' \
    -derivedDataPath "$DERIVED_DATA" \
    CURRENT_PROJECT_VERSION="$BUILD_NUMBER" \
    CODE_SIGNING_ALLOWED=NO CODE_SIGNING_REQUIRED=NO build
  check_app "$DERIVED_DATA/Build/Products/Release-iphoneos/$APP_NAME"
  printf 'Unsigned iPhone app ready (build %s)\n' "$BUILD_NUMBER"
}

archive() {
  require_tools
  prepare_project
  prepare_auth
  rm -rf "$ARCHIVE_PATH"
  xcodebuild -project "$PROJECT" -scheme "$SCHEME" -configuration Release \
    -sdk iphoneos -destination 'generic/platform=iOS' \
    -derivedDataPath "$DERIVED_DATA" -archivePath "$ARCHIVE_PATH" \
    "${AUTH_ARGS[@]}" \
    DEVELOPMENT_TEAM="$TEAM_ID" CODE_SIGN_STYLE=Automatic \
    CURRENT_PROJECT_VERSION="$BUILD_NUMBER" archive
  check_app "$ARCHIVE_PATH/Products/Applications/$APP_NAME"
  printf 'Signed archive ready (build %s): %s\n' "$BUILD_NUMBER" "$ARCHIVE_PATH"
}

export_archive() {
  local options="$1"
  check_app "$ARCHIVE_PATH/Products/Applications/$APP_NAME"
  prepare_auth
  rm -rf "$EXPORT_PATH"
  xcodebuild -exportArchive -archivePath "$ARCHIVE_PATH" \
    -exportPath "$EXPORT_PATH" -exportOptionsPlist "$options" "${AUTH_ARGS[@]}"
}

case "${1:-}" in
  preflight) preflight ;;
  archive) archive ;;
  export) export_archive "$EXPORT_OPTIONS" && printf 'IPA exported under %s\n' "$EXPORT_PATH" ;;
  upload)
    archive
    export_archive "$UPLOAD_OPTIONS"
    printf 'Build %s submitted to App Store Connect.\n' "$BUILD_NUMBER"
    ;;
  *)
    printf 'Usage: %s {preflight|archive|export|upload}\n' "$0" >&2
    exit 2
    ;;
esac
