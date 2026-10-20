#!/usr/bin/env bash
# Native Apple checks. Test doubles prove fail-closed orchestration, not signing.
set -euo pipefail
umask 077

script_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
verify_only=false
if [ "${1:-}" = '--verify-only' ]; then
  verify_only=true
  shift
fi
artifact=${1:?artifact path required}
kind=${2:?app, dmg or bin required}
case "$kind" in
  app) test -d "$artifact" ;;
  dmg) test -f "$artifact" ;;
  # A bare command-line executable: notarized via a zip, but Apple cannot staple
  # a ticket to it and Gatekeeper's assessment does not apply.
  bin) test -f "$artifact"; : "${BIN_IDENTIFIER:?BIN_IDENTIFIER is required for bin}" ;;
  *) echo 'expected app, dmg or bin' >&2; exit 1 ;;
esac
if [[ ! ${TEAM_ID:-} =~ ^[A-Z0-9]{10}$ ]]; then
  echo 'a ten-character Apple TEAM_ID is required' >&2
  exit 1
fi

verify_signature() {
  local requirement="anchor apple generic and certificate leaf[subject.OU] = \"$TEAM_ID\""
  if [ "$kind" = app ]; then
    requirement="$requirement and identifier \"com.symaira.brain\""
  elif [ "$kind" = bin ]; then
    requirement="$requirement and identifier \"$BIN_IDENTIFIER\""
  fi
  codesign --verify --deep --strict --verbose=2 "$artifact"
  # A leading "=" makes codesign read the requirement as text, not as a file path.
  codesign --verify --strict --verbose=2 -R "=$requirement" "$artifact"
}

verify_signature
if [ "$verify_only" = false ]; then
  receipt_prefix=${3:?owned receipt prefix required}
  : "${NOTARY_API_KEY_ID:?}" "${NOTARY_API_ISSUER_ID:?}" "${API_KEY_PATH:?}"
  submission=$artifact
  if [ "$kind" != dmg ]; then
    submission="$receipt_prefix.zip"
    ditto -c -k --sequesterRsrc --keepParent "$artifact" "$submission"
  fi
  xcrun notarytool submit "$submission" \
    -d "$NOTARY_API_KEY_ID" -i "$NOTARY_API_ISSUER_ID" -k "$API_KEY_PATH" \
    --wait --output-format json > "$receipt_prefix-submit.json"
  submission_id=$(python3 "$script_root/verify-notary-receipt.py" "$receipt_prefix-submit.json")
  xcrun notarytool info "$submission_id" \
    -d "$NOTARY_API_KEY_ID" -i "$NOTARY_API_ISSUER_ID" -k "$API_KEY_PATH" \
    --output-format json > "$receipt_prefix-info.json"
  python3 "$script_root/verify-notary-receipt.py" "$receipt_prefix-info.json" \
    --expected-id "$submission_id" > /dev/null
  if [ "$kind" != bin ]; then
    xcrun stapler staple "$artifact"
  fi
fi
if [ "$kind" = bin ]; then
  verify_signature
  exit 0
fi
xcrun stapler validate "$artifact"
verify_signature
if [ "$kind" = app ]; then
  spctl --assess --type execute --verbose=2 "$artifact"
else
  spctl --assess --type open --context context:primary-signature --verbose=2 "$artifact"
fi
